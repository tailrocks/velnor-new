# Generator configuration

The new Rust generator reads `.velnor/rust-ci.toml`.

## Fork pull requests

A pull request from a fork must never run on a self-hosted runner. When `runner` is not a GitHub-hosted label, the generator renders `runs-on` of every job, including the `Required` gate, as an expression. It selects `runner` when the event is not a pull request or the pull request head repository is this repository, and `fork_runner` otherwise. No job is skipped for a fork, so the gate sees the real result of every job and fails when one fails. A deleted fork has no head repository, so it counts as a fork.

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

## Test setup steps

A test that needs a fixture, such as a built example executable or a provisioned service, declares one step per fixture. The generator runs each step before the first test step of every job of that root, in the order written. Each step is one command, because a generated `run:` value is one command with no shell logic; the logic lives in a script the repository commits. The script exports variables to later steps through `$GITHUB_ENV`.

```toml
[[test_setup]]
root = "."
name = "Prepare the fixtures"
run = "bash scripts/ci-fixtures.sh"
```

`root` is the folder of the Cargo root's `Cargo.toml`, or `.` for the repository root, and it must name a root of this repository. The step runs in that folder. It follows the same gate as the test steps, so a run that selects no tests skips it. A job that runs no tests has none. With `shape = "groups"` the test group job installs only cargo-nextest, so a step there must not need Rust. A `run` value with an expression, a line break or any shell syntax is refused.

## Optional reusable workflows

`.velnor/workflows.toml` makes the generator write one reusable workflow, `.github/workflows/<file>.yml`, for each `[[workflow]]` table. It is off when the file is absent, and then the generated files are byte-identical to the output without this feature. The workflow is called by another workflow, so it has the `workflow_call` trigger and no other trigger. It sets `permissions: {}` at the top, and each job lists only the scopes it names. The file names are `file = "native-checks"` and so on; `rust-ci` and `release` belong to other outputs and are refused. The samples `reusable-workflows` and `declared-workflow` use every key below.

```toml
[[workflow]]
file = "native-checks"
name = "Native checks"
runner = "ubuntu-latest"

[[workflow.input]]
name = "sdk_version"
description = "SDK version to check"
required = true

[[workflow.job]]
id = "plan"
helper = true
timeout_minutes = 15
permissions = { contents = "read" }
outputs = { matrix = "steps.plan.outputs.matrix" }

[[workflow.job.step]]
id = "plan"
name = "Plan the checks"
run = "velnor-ci plan"
env = { SDK_VERSION = "${{ inputs.sdk_version }}" }

[[workflow.job]]
id = "check"
needs = ["plan"]

[workflow.job.matrix]
from_output = "plan.matrix"
fail_fast = false
max_parallel = 2

[[workflow.job.step]]
name = "Warm the cache"
run = "velnor-ci warm"
continue_on_error = true
env = { TARGET = "${{ matrix.target }}" }
```

- Inputs. Each `[[workflow.input]]` becomes a typed `string` input of `workflow_call`. `required` is `true` unless you set it to `false`.
- Matrix from a job output. A job with `[workflow.job.matrix]` renders `strategy.matrix: ${{ fromJSON(needs.<job>.outputs.<output>) }}`. `from_output` is `<job>.<output>`; that job must be in the job's `needs` and must declare the output. `fail_fast` and `max_parallel` are optional.
- Job outputs. `outputs` maps a name to `steps.<step id>.outputs.<name>` of a step of the same job.
- Per-step `continue_on_error = true` renders `continue-on-error: true` on that step only.
- `helper = true` puts the pinned `velnor-ci` install step first in the job.
- A job can set `environment = "<name>"`. `permissions` accepts these scopes: `actions`, `artifact-metadata`, `attestations`, `checks`, `contents`, `deployments`, `id-token`, `issues`, `packages`, `pages`, `pull-requests`, `security-events` and `statuses`, each `read`, `write` or `none` (`id-token` has no `read`).
- A workflow can set `if = "<expression>"`, with or without `${{ }}`. It is added to every job's `if`.
- A step is exactly one of `run`, `uses` and `helper`. `uses` names a SHA-pinned action from this list: `checkout`, `mise`, `upload-artifact`, `download-artifact`, `cache-restore` and `cache-save`; `with` sets its inputs. `helper = "<command>"` renders `run: velnor-ci <command>` and needs `helper = true` on its job. A step can set `if`; a name is optional for `uses` and `helper` steps.
- An `if` may read `needs.<needed job>.result`, `steps.<earlier step id>.outcome` and `github.*`, besides what a value may read. An expression that names a job missing from `needs`, a later or unknown step, or any other context (such as `secrets`) is refused.
- Every `run` follows the same rules as the other generated workflows: one command, no shell logic, no expression. Pass values through `env`.
- An `env` value is plain text or exactly one expression. The allowed expressions are `github.token`, `github.repository`, `github.sha`, `inputs.<declared input>`, `matrix.<key>` in a matrix job, `steps.<earlier step id>.outputs.<name>` and `needs.<needed job>.outputs.<declared output>`. Anything else, and any `needs` on a job declared later, is refused with the path of the problem.

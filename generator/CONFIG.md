# Generator configuration

The new Rust generator reads `.velnor/rust-ci.toml`.

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

## Declared workflows

`[[workflow]]` in `.velnor/rust-ci.toml` declares one extra workflow file of fixed shape, such as an authority workflow with its own jobs, needs, outputs and environments. It is off when the table is absent, and then the generated files are unchanged. The file is written to `.github/workflows/<file>` and must not be a file another generated workflow owns.

```toml
[[workflow]]
name = "Preview authority"
file = "preview-authority.yml"
on = "workflow_call"
permissions = { contents = "read" }
if = "github.event_name == 'push' && startsWith(github.ref, 'refs/tags/sdk-preview-')"

[[workflow.job]]
id = "draft"
needs = ["source"]
environment = "generator-source-preview"
permissions = { actions = "read", contents = "write" }
timeout_minutes = 10
outputs = { sign = "steps.draft.outputs.sign" }

[[workflow.job.step]]
id = "draft"
helper = "validate-artifact-id"
if = "needs.source.result == 'success'"
env = { ARTIFACT_ID = "${{ needs.source.outputs.artifact_id }}" }
```

- `on` is one event or a list of `workflow_call`, `workflow_dispatch`, `push`, `pull_request` and `merge_group`, without filters. Top-level `permissions` defaults to none.
- `if` on the workflow gates every job. A job's own `if` is combined with it by `&&`. Write conditions and `outputs` values without `${{ }}`.
- A job runs on the repository `runner` and takes `timeout_minutes` from the job or the repository default (1 to 360). `needs` must name jobs declared earlier in the file, so a cycle cannot be written.
- A step is `uses` or `helper`. `uses` names a pinned action of the fixed list (`checkout`, `mise`, `upload-artifact`, `download-artifact`, `cache-restore`, `cache-save`); the configuration never carries a SHA or a free `uses`. `helper` names one `velnor-ci` command and renders as `run: velnor-ci <name>`. A `run` key is refused, because a generated `run:` value is never a free shell string.
- `with` and `env` values are literals or contain `${{ }}` expressions. `outputs`, `if`, `with` and `env` may read only `needs.<job>.outputs.*` for a listed `needs` entry, `steps.<id>.outputs.*` for a step id of the same job, `job.*` and `github.token`. A condition may also read `github.*`, `needs.*.result` and `steps.<id>.outcome`. Any other context, such as `secrets`, `vars` or `env`, is refused.
- Every step id and job id is unique, and every `steps.<id>` an expression reads must be declared in that job.

The helper must provide each named command; the generator does not check that the pinned `velnor-ci` release knows it.

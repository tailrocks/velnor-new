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

## Optional private dependencies

A project whose crates depend on private repositories names them in `.velnor/rust-ci.toml`. The table is off when absent, and then the generated workflow is unchanged.

```toml
[private_sources]
app_id = "vars.DEPS_APP_ID"
private_key = "secrets.DEPS_APP_PRIVATE_KEY"
repositories = ["owner/shared-lib"]
```

`velnor-gen generate` prints a note that suggests this table when `Cargo.lock` locks a dependency from a registry other than crates.io, from a git host that is not github.com, gitlab.com, codeberg.org or bitbucket.org, over ssh or with credentials in the URL, or from a github.com owner that is the owner of this repository. It writes no secret, and it stays silent for crates.io and for public git dependencies of other owners.

`app_id` must be a `vars.` name and `private_key` a `secrets.` name. `repositories` is a non-empty list of `owner/name` entries with one owner. Anything else is refused. The key needs a helper release of `0.1.86` or later.

Each job that compiles gets two steps after `jdx/mise-action` and before its first compiling cargo command. The first creates a read-only token for the listed repositories with `actions/create-github-app-token`. It does not run for pull requests from forks. The second runs `velnor-ci fetch` with the token in its step environment only. `velnor-ci fetch` runs `cargo fetch --locked` for each Cargo root, gives the token to git through `GIT_CONFIG_*` variables of that command alone, and then revokes it with `DELETE /installation/token`. It starts `cargo` and `git` by absolute path from the tool directories the runner installed, with an environment it builds itself, so a changed `mise.toml` cannot redirect the token. It refuses a `CARGO_HOME` outside those directories. Without a token it runs `cargo fetch --locked --offline` and fails with a message that names the fork case when the caches lack a dependency.

Every later cargo command of the job runs with `--offline` and `CARGO_NET_OFFLINE=true`. The token is written to no file, to `$GITHUB_ENV` or to any git configuration.

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

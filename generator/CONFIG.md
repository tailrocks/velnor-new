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

## CI on a pull request only on request

`pr_ci = "label"` in `.velnor/rust-ci.toml` turns the full suite on a pull request into something a maintainer asks for. It is `"always"` by default, and then the generated workflow is byte-identical to one without the key. The key needs a helper release that knows it (`0.1.82` or later), because older helpers reject unknown keys, and it cannot be combined with `skip_draft_pull_requests`, which also decides what a draft runs.

```toml
pr_ci = "label"
```

| Event | What runs | `Required` |
|---|---|---|
| Draft pull request | Nothing | Passes with the notice `draft: CI not requested` |
| Ready pull request | The quick set: the plan job, the lint jobs (in a one-job workflow, everything but the test steps) and the policy job | Passes when the quick set passes; the gate is not run |
| Ready pull request with the label `ci:run` | The full suite once for this commit; a job removes the label as the run starts | The gate, as without `pr_ci` |
| Ready pull request with the label `ci:auto` | The full suite on every push; the label stays | The gate, as without `pr_ci` |
| `merge_group`, push to the default branch, schedule, `workflow_dispatch` | The full suite, exactly as without `pr_ci` | The gate, as without `pr_ci`; a skipped lane is never accepted |

A draft runs nothing even when it carries a label; the label takes effect when the pull request is marked ready, which starts a run. The pull request events are `opened`, `synchronize`, `reopened`, `ready_for_review`, `edited` (it also fires when the base branch is retargeted, so a result for the old base is replaced) and `labeled`. The workflow cannot filter by label name, so every added label starts a run, but a run for a label other than `ci:run` and `ci:auto` runs nothing: all its jobs, `Required` included, are skipped, and it gets a concurrency group of its own (`-ignored-<run id>` is appended), so it cannot cancel the run in progress.

The job `ci-run-label` removes `ci:run` as soon as the run starts (it has no `needs`), so `ci:run` means one full run of that commit and a new push needs a new `ci:run`. The run in progress still runs the full suite, because every condition reads the labels from the event payload, which holds them as they were when the run was triggered. Removing a label starts no run: `unlabeled` is not a trigger type. It is the only job with `pull-requests: write`. It has that permission alone, no checkout and one `curl` step, so it runs no code of the pull request. It is skipped on a draft and on a pull request from a fork, whose token cannot write. A pull request from a fork never runs the full suite on a label, whoever applies it: it gets the quick set, and the full suite runs when it reaches the merge queue. Keeping a fork's code off self-hosted runners stays with the fork fence.

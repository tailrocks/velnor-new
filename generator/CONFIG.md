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

## Test groups

Heavy or special tests get their own job; every other test stays in the default job. A group names a nextest filterset, what its tests need and the limits of its job.

```toml
[[test_groups]]
name = "database"
filter = "package(app) and test(/^db_/)"
needs = ["docker"]
test_threads = 2
timeout_minutes = 30
runner = "ubuntu-latest"
root = "."
```

- `name` is the job name `rust-test-<name>` (then the root suffix when the repository has several roots). It uses letters, digits, `_` and `-`, at most 40 characters, and is not `default`.
- `filter` is a nextest filterset. The group job runs the tests the plan selects that this filter matches. Write nextest's `and`, `or` and `not` instead of `&`, `|` and `!`: a generated `run:` value is one command with no `&`, `|`, `;`, quote, `$`, backtick, backslash, `<` or line break. An empty filter is refused.
- `needs` lists what the job needs before the tests start. `"docker"` adds a step that runs `docker info`, so a runner without Docker fails there. `"cargo"` installs the full toolchain, as the build job does, but restores no target or sources cache. Nothing else is installed: a group with no need installs only cargo-nextest. A test that calls `cargo` may print `Compiling` for its own build, and it must not rebuild the suite.
- `test_threads` (1 to 256) becomes `--test-threads` for that job only. `timeout_minutes` (1 to 360) and `runner` replace the repository values for that job only.
- `root` is the folder of the Cargo root's `Cargo.toml`, or `.`. It is required when the repository has several Cargo roots, and it must name a root with nextest tests.
- `paths` and `events` are in the model but refused for now, because choosing groups at run time needs the plan. A group runs whenever its root is selected.

A repository with any `[[test_groups]]` splits each root that has a group into the archive build job and the test jobs, whatever `shape` is: the group jobs run from the archive of the build job (`shape = "groups"`), and lint and doctests stay on the build job. The default job `rust-test-default` leaves out every group's tests with `and not (<filter>)`, and every group job is a need of the `Required` check. Two groups with one name in one root are refused. The key needs `velnor-ci` 0.1.82 or later, because the helper reads the same file.

`velnor-gen suggest-groups` reads `.velnor/rust-ci-timings.json` and prints a `[[test_groups]]` block for every test binary that takes more than a third of its root's total test-binary time, slowest first and by name on a tie. Each block has a name made from the binary id, `filter = "binary_id(=<id>)"` and, when the repository has several Cargo roots, `root`. A binary that a group of the same root already filters by is left out. The command writes no file, needs no pinned `velnor-ci`, and prints nothing on standard output when no binary is that slow.

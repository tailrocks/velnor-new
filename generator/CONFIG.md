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

## Optional extra binaries and files in a release asset

A `[[binaries]]` entry in `.velnor/release.toml` packs one binary per platform by default: `<bin>-<platform>.tar.gz` holds only the file `<bin>`. Two opt-in keys put more in the same asset.

```toml
[[binaries]]
package = "velnor-runner-cli"
bin = "velnor-host"
bins = [{ package = "velnor-runner-attestation", bin = "velnor-runner-attestation-helper" }]
include = ["LICENSE", "NOTICE"]
```

`bins` lists further binaries of the same release root. Each entry names a `bin` and optionally its `package`, which defaults to the entry's package. They are built by the same `cargo build` command as the main binary. `include` lists tracked files, as paths relative to the repository root, that are copied into the asset.

Every entry lands at the top level of the asset, flat and by its file name: the main binary, then `bins` in the order written, then `include` in the order written. This is the layout of the old engine's `velnor-host` release. A `bin` or file name that appears twice, an `include` path that is absolute, starts with `-`, contains `..` or a character other than letters, digits, `.`, `_`, `-` and `/`, a `bin` that no package of the root builds, and an expression in any value are refused at generation. A listed file that is missing from the checkout fails the build job, not generation.

Both keys are absent by default, and then the generated workflow is byte-identical to one generated without this feature. The `SHA256SUMS` file still covers the archives, not their members.

## Release binary asset format

`format` in a `[[binaries]]` table of `.velnor/release.toml` selects what the release job uploads for that binary. It is `"archive"` by default, and then the output is unchanged: one musl `<bin>-<platform>.tar.gz` for each platform plus the `SHA256SUMS` file.

`format = "raw"` publishes the bare executable instead, built for the gnu target (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`), so a consumer can download it by its exact name. For each platform (`linux-x64`, `linux-arm64`, `macos-arm64`) the release holds three assets:

- `<bin>-<platform>`: the executable, for example `velnor-ci-linux-x64`
- `<bin>-<platform>.sha256`: one line, `<sha256>  <bin>-<platform>`, as `sha256sum --check` reads it
- `<bin>-<platform>.provenance.json`: `schema`, `tag`, `repository`, `commit`, `target`, `asset` and `sha256`

The release also carries `SHA256SUMS`, which lists all of these files, because `velnor-ci release publish` checks every file it uploads against it. The gnu build links the glibc of the runner image, so a consumer needs a glibc at least that recent.

```toml
[[binaries]]
package = "velnor-ci"
format = "raw"
```

`bins` and `include` have no meaning for a raw binary, which publishes only the main executable, so `format = "raw"` with either is refused at generation.

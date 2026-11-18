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

## Optional release asset names

A `[[binaries]]` entry in `.velnor/release.toml` publishes `<bin>-<platform>.tar.gz` by default, for example `app-linux-x64.tar.gz`. `asset_name` replaces the name without the `.tar.gz` suffix, so a consumer that expects an exact file name, such as a package manager recipe, keeps it. The key is optional, and an entry without it generates the same workflow as before.

```toml
[[binaries]]
package = "app"
platforms = ["linux-x64", "macos-arm64"]
asset_name = "{{ bin }}-{{ version }}-{{ target }}"
```

The placeholders are `{{ bin }}`, `{{ platform }}` (`linux-x64`), `{{ target }}` (the Rust target triple the job builds, such as `x86_64-unknown-linux-musl`) and `{{ version }}`. Write them exactly as shown, with one space inside the braces. The template must contain `{{ platform }}` or `{{ target }}`, so each platform gets its own file, may use only letters, digits, `.`, `_` and `-` besides the placeholders, and must start with a letter or digit. Anything else is refused at generation.

`{{ version }}` is the version in the released tag. The build, `SHA256SUMS` and publish jobs each read it from the tag with a step that sets `VERSION` through `$GITHUB_ENV`, and the build job packs into a `stage` folder that it uploads. The tag name must end in the version, as the default `{{ package }}-v{{ version }}` does. A template without `{{ version }}` adds no step and no folder.
## Optional build-provenance attestation of release binaries

`attest = true` on a `[[binaries]]` entry in `.velnor/release.toml` makes the generated `release.yml` attest that binary's release assets. It is `false` by default, and then the workflow is byte-identical to one generated without the key.

```toml
[[binaries]]
package = "app"
attest = true
```

The `sums-<package>` job, which already downloads every platform archive and writes `SHA256SUMS`, gets one `actions/attest-build-provenance` step (pinned by commit) with `subject-path: dist/*`. It runs after the checksums are written and before they are uploaded, so the attestation covers every archive and `SHA256SUMS`. That job, and only that job, holds `id-token: write` and `attestations: write`; the workflow level keeps `permissions: {}`, and the build and publish jobs do not gain either scope. The build jobs run the compiler and stay without a token that can mint an identity. Verify a downloaded asset with `gh attestation verify <file> --repo <owner>/<repo>`.

With `attest = true` a separate `verify-<package>` job verifies every archive and `SHA256SUMS`, and `publish-<package>` needs it. One step per file runs `gh attestation verify` with a pinned `gh` (installed through mise, the same version the product release workflows use), and checks `--repo`, `--source-digest`, `--source-ref`, `--signer-workflow` (this `release.yml`) `--signer-digest` and `--deny-self-hosted-runners`. A failed verification fails the job, `publish-<package>` is skipped and nothing is published. The `verify-<package>` job holds only `attestations: read`, with no `contents` scope. Its `GITHUB_TOKEN` is the one the pinned `gh` sees, so that binary can read attestations and nothing else. The `publish-<package>` job holds only `contents: write`, as it does without `attest`, and it never installs `gh` or runs mise. Both jobs download the artifacts separately, so a swap of an artifact between the two jobs is not detected.

`--source-digest` and `--source-ref` are the `GITHUB_SHA` and `GITHUB_REF` of a `workflow_run` job, which are the default-branch tip when the run started and not the tag commit. The archives are built from the tag, and the build jobs already check that the tag is the commit the gate verified. The attestation therefore proves that this workflow attested the files on a hosted runner at that default-branch commit. It does not prove that the files were built from the tag commit. `--deny-self-hosted-runners` rejects an attestation made on a self-hosted runner, and the `sums-<package>` job runs on a hosted runner, so no legitimate attestation is rejected.

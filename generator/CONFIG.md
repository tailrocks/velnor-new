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

With `bins`, each extra binary is published the same way, as its own `<bin>-<platform>` asset with its own `.sha256` and `.provenance.json`, and `SHA256SUMS` lists them all. `include` has no meaning for a raw binary, because the release holds no archive to put a file in, so `format = "raw"` with `include` is refused at generation.

## Release checks

A repository that must pass its own checks before a release publishes declares one `[[checks]]` entry per check in `.velnor/release.toml`. Without an entry the generated `release.yml` is unchanged. Each entry becomes one job of `release.yml`, named `check-<name>` with the name lowercased and every run of other characters turned into `-`.

```toml
[[checks]]
name = "Qualify linux"
run = "bash scripts/qualify-linux.sh"
before = "publish"
artifacts = ["app-linux-x64"]
needs = ["Verify release inputs"]
runs_on = "ubuntu-24.04"
timeout_minutes = 20
[checks.permissions]
actions = "read"
```

- `name` and `run` are required. `run` is one command, with the same limits as every generated `run:` value: no line break, no shell syntax and no expression. The logic lives in a script the repository commits.
- `before` is `"tag"` (the default) or `"publish"`. A check that runs before the tag waits for the gate, and the `tag` job waits for it, so a failing check stops the release before release-plz tags or publishes a crate. A check that runs before publish waits for the gate, the `tag` job and the builds of its artifacts, and the `publish-<package>` job of every package it names waits for it, so a failing check leaves that release a draft.
- `artifacts` lists the build artifacts a publish-stage check qualifies, each as `<package>-<platform>` of a `[[binaries]]` entry, such as `app-linux-x64`. They are downloaded into `dist` before `run`. A publish-stage check must name at least one, and a tag-stage check must name none.
- `needs` lists the names of checks declared earlier in the file. A tag-stage check cannot need a publish-stage check.
- `runs_on` is a runner label and defaults to `ubuntu-latest`. `timeout_minutes` defaults to 30 and is at most 360.
- `permissions` adds token scopes to the job. The job holds `contents: read` and nothing else unless the table declares more. `contents: write` is refused, because the job runs repository code.

A check job checks out the verified commit without persisting credentials, and runs without the `release` environment, so it holds no release App secret. A check cannot use `uses:`, so a step that needs an action must be a script step or a separate workflow.


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

With `attest = true` the `publish-<package>` job also verifies every archive and `SHA256SUMS` before it publishes. One step per file runs `gh attestation verify` with a pinned `gh` (installed through mise, the same version the product release workflows use), and checks `--repo`, `--source-digest`, `--source-ref`, `--signer-workflow` (this `release.yml`) and `--signer-digest`. A failed verification fails the job and nothing is published. The job gains only `attestations: read`.


`asset_name` applies to `format = "raw"` too: the template names the executable, and the `.sha256` and `.provenance.json` files take the same name. With `bins` on a raw binary, each extra binary is named by the same template with its own `{{ bin }}`, so the template must contain `{{ bin }}` or the assets would collide, and a template without it is refused. In an archive, `{{ bin }}` is the main binary.

# Generator configuration

The new Rust generator reads `.velnor/rust-ci.toml`.

## Fork pull requests

By default, a pull request from a fork does not select a self-hosted runner. When `runner` is not a GitHub-hosted label, the generator renders `runs-on` of every job, including the `Required` gate, as an expression. It selects `runner` when the event is not a pull request or the pull request head repository is this repository, and `fork_runner` otherwise. No job is skipped for a fork, so the gate sees the real result of every job and fails when one fails. A deleted fork has no head repository, so it counts as a fork.

`fork_runner` is a GitHub-hosted label. It defaults to `ubuntu-latest`, to `ubuntu-24.04-arm` when `runner` is a Linux arm64 label and to `macos-15` when `runner` is an Apple silicon macOS label. The generator refuses a `fork_runner` that is not GitHub-hosted or that does not match the platform of `runner`. When the platform of `runner` is unknown, which is the case for a label such as `velnor-box`, the generator accepts only a `fork_runner` that is linux-x64, so `windows-latest`, `macos-15` and `ubuntu-24.04-arm` are refused until the platform is known. It also refuses a self-hosted `runner` label that holds anything beyond letters, digits, `-`, `_` and `.`, because the label is written into an expression.

The fence does not stop a fork pull request that edits its own copy of the workflow. The real boundary is outside this file: first, the repository setting Settings > Actions > General > Fork pull request workflows, set to "Require approval for all outside collaborators"; second, a self-hosted runner group whose repository access is limited to this repository, so no other repository's workflows can target its runners. A repository with a self-hosted `runner` has no opt-out from the fence.

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

`mbx_shared_store = true` in `.velnor/rust-ci.toml` makes every job run `velnor-ci mbx-store` right after the helper is installed. It needs `velnor-ci` 0.1.85 or later and is off by default.

The runner host may set `VELNOR_MBX_SHARED_DIR` in the runner pod. The workflow `env` context cannot see the runner process environment, so the helper decides at run time. On a trusted event (a push or schedule on the default branch), when the directory is absolute, exists, is not a symlink and contains no symlink, and is writable, the helper appends `MBX_CACHE_DIR=<dir>` and a pod-private `MBX_SHIMS_DIR=$RUNNER_TEMP/velnor-mbx-shims` to `$GITHUB_ENV`. A pull request, a fork, a `merge_group` run, or any other event never gets the shared directory. A directory that fails a check produces a `::warning::` and the job keeps MBX's own per-job store. Without the variable the step changes nothing.

With `velnor-ci` 0.1.86 or later in the lock, `mbx_shared_store = true` also renders a `velnor-ci mbx-seed` step right after `mbx-store`, with `if:` set to pull_request or merge_group. The runner host may mount the trusted directory read-only and set `VELNOR_MBX_SEED_DIR` to it. When that directory is absolute, exists, is not a symlink, contains no symlink on its path, is owned by the runner user or root and is not world-writable, the helper copies the missing files of `actions/cas`, `actions/executable-identities`, `actions/task-manifests` and `actions/action-results` into the private store `$RUNNER_TEMP/velnor/mbx`, in parallel, and appends `MBX_CACHE_DIR=<private store>` to `$GITHUB_ENV`. It logs the file count, bytes and seconds. It never writes to the seed and never follows a symlink inside it. Without the variable, with an empty seed, or when a check fails (with a `::warning::`), it changes nothing and MBX builds cold. With an older helper the step is not rendered and pull requests stay cold.

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

A `[[binaries]]` entry in `.velnor/release.toml` packs one binary per platform by default: `<bin>-<target-triple>.tar.gz` holds only the file `<bin>`, as the archives of uv, ripgrep and bat do. Two opt-in keys put more in the same asset.

```toml
[[binaries]]
package = "velnor-runner-cli"
bin = "velnor-host"
bins = [{ package = "velnor-runner-attestation", bin = "velnor-runner-attestation-helper" }]
include = ["LICENSE", "NOTICE"]
```

`bins` lists further binaries of the same release root. Each entry names a `bin` and optionally its `package`, which defaults to the entry's package. They are built by the same `cargo build` command as the main binary. `include` lists tracked files, as paths relative to the repository root, that are copied into the asset.

Every entry lands at the top level of the asset, flat and by its file name: the main binary, then `bins` in the order written, then `include` in the order written. This is the layout of the old engine's `velnor-host` release. A `bin` or file name that appears twice, an `include` path that is absolute, starts with `-`, contains `..` or a character other than letters, digits, `.`, `_`, `-` and `/`, a `bin` that no package of the root builds, and an expression in any value are refused at generation. A listed file that is missing from the checkout fails the build job, not generation.

Both keys are absent by default, and then the generated workflow is byte-identical to one generated without this feature. The `SHA256SUMS` file still covers the archives, not their members. With `attest = true` each archive is its own attestation subject, so the binary, the helper, `LICENSE` and `NOTICE` travel in one verified file.

## Release binary asset format

`format` in a `[[binaries]]` table of `.velnor/release.toml` selects what the release job uploads for that binary. It is `"archive"` by default: one `<bin>-<target-triple>.tar.gz` for each platform plus the `SHA256SUMS` file.

| Platform | Archive asset (default, musl) | Runner |
|---|---|---|
| `linux-x64` | `<bin>-x86_64-unknown-linux-musl.tar.gz` | `ubuntu-24.04` |
| `linux-arm64` | `<bin>-aarch64-unknown-linux-musl.tar.gz` | `ubuntu-24.04-arm` |
| `macos-arm64` | `<bin>-aarch64-apple-darwin.tar.gz` | `macos-15` |

`target = "gnu"` builds the linux platforms for `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu` and names the archives after those triples. The macOS archive does not change.

`checksum_files = "sha256"` adds a `<archive>.sha256` file next to each archive. It holds one line, `<sha256>  <archive>`, as `sha256sum --check` reads it. `SHA256SUMS` lists the sidecars too, because `velnor-ci release publish` checks every file it uploads against it, and an attested release attests and verifies them. It is off by default, and refused with `format = "raw"`, which always publishes its sidecar.

`member_digests = "sha256"` adds a `BINARY-SHA256SUMS` asset that lists the digest of each executable inside each archive, one `<sha256>  <archive>/<member>` line per archive and per `bin` or `bins` entry. The `sums-<package>` job unpacks the archives it downloaded and hashes the members, so a digest never comes from a consumer. It writes the file before `SHA256SUMS`, so `SHA256SUMS` lists it, an attested release attests and verifies it, and `velnor-ci release publish` uploads it. `velnor-gen pin-helper` downloads it when `SHA256SUMS` lists it, checks it against that listing, and records both digests in `.velnor/rust-ci.lock`: the archive digests under `[velnor-ci]`, which `actions/setup-velnor` checks before it unpacks anything, and the executable digests under `[velnor-ci-binary]`, which the generated launcher checks before every run. A lock without `[velnor-ci-binary]` still works, and then both checks use the `[velnor-ci]` digest. It is off by default, and refused with `format = "raw"`.

`format = "raw"` publishes the bare executable instead, built for the gnu target (`x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`) unless `target = "musl"` says otherwise, so a consumer can download it by its exact name. For each platform (`linux-x64`, `linux-arm64`, `macos-arm64`) the release holds three assets:

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

### Raw binary keys

Three keys of a raw `[[binaries]]` entry add what the old `release-rust-ci-tools.yml` did. All are off by default, and then the generated workflow is byte-identical to one generated without them.

```toml
[[binaries]]
package = "velnor-ci"
format = "raw"
target = "musl"
smoke = true
commit_file = true
```

- `target = "musl"` builds `<arch>-unknown-linux-musl` for the linux platforms of a raw binary, with the musl C compiler installed, so the executable is static. `macos-arm64` keeps `aarch64-apple-darwin`. The default for raw stays `"gnu"`. `target = "musl"` is refused without a linux platform. The archive format accepts `target` too and defaults to musl.
- `smoke = true` adds two steps to each build job after packing, for the main binary of each platform: `./<asset> --version`, then `./<asset> check-tag "$TAG"`, where `TAG` is the tag the release records for the package. The binary must implement `check-tag <tag>`, which fails when the tag does not name the version of the binary. `smoke` is refused with the archive format.
- `commit_file = true` adds a `COMMIT` asset holding the commit the tag is on. The `sums-<package>` job writes it before `SHA256SUMS`, so `SHA256SUMS` lists it, an attested release attests and verifies it, and `velnor-ci release publish` uploads it.

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

A `[[binaries]]` entry in `.velnor/release.toml` publishes `<bin>-<target-triple>.tar.gz` by default, for example `app-x86_64-unknown-linux-musl.tar.gz`. `asset_name` replaces the name without the `.tar.gz` suffix, so a consumer that expects an exact file name, such as a package manager recipe, keeps it. The key is optional, and an entry without it generates the same workflow as before.

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

A re-run of only the verify or publish job can fail. The artifacts expire after one day, and the attestation is checked against the commit of that run. The failure fails closed: nothing is published. Re-run the whole workflow run, or let the next default-branch run release.


`asset_name` applies to `format = "raw"` too: the template names the executable, and the `.sha256` and `.provenance.json` files take the same name. With `bins` on a raw binary, each extra binary is named by the same template with its own `{{ bin }}`, so the template must contain `{{ bin }}` or the assets would collide, and a template without it is refused. In an archive, `{{ bin }}` is the main binary.
## CI on a pull request only on request

`pr_ci = "label"` in `.velnor/rust-ci.toml` turns the full suite on a pull request into something a maintainer asks for. It is `"always"` by default, and then the generated workflow is byte-identical to one without the key. The key needs a helper release that knows it (`0.1.86` or later), because older helpers reject unknown keys, and it cannot be combined with `skip_draft_pull_requests`, which also decides what a draft runs.

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

A draft runs nothing even when it carries a label; the label takes effect when the pull request is marked ready, which starts a run. The pull request events are `opened`, `synchronize`, `reopened`, `ready_for_review`, `edited` and `labeled`. An `edited` event runs only when it retargets the base branch (`github.event.changes.base` is present), so a result for the old base is replaced; an edit of the title or body runs nothing, so it cannot cancel a requested full run. The workflow cannot filter by label name, so every added label starts a run, but a run for a label other than `ci:run` and `ci:auto` runs nothing: all its jobs, `Required` included, are skipped, and it gets a concurrency group of its own (`-ignored-<run id>` is appended), so it cannot cancel the run in progress. An `edited` event that does not change the base is treated the same way.

The job `ci-run-label` removes `ci:run` as soon as the run starts (it has no `needs`), so `ci:run` means one full run of that commit and a new push needs a new `ci:run`. The run in progress still runs the full suite, because every condition reads the labels from the event payload, which holds them as they were when the run was triggered. Removing a label starts no run: `unlabeled` is not a trigger type. It is the only job with `pull-requests: write`. It has that permission alone, no checkout and one `curl` step, so it runs no code of the pull request. It is skipped on a draft and on a pull request from a fork, whose token cannot write. A pull request from a fork never runs the full suite on a label, whoever applies it: it gets the quick set, and the full suite runs when it reaches the merge queue. Keeping a fork's code off self-hosted runners stays with the fork fence.

## Release binaries and their Cargo profile

`.velnor/release.toml` lists the binaries that `release.yml` builds, packs and publishes, one `[[binaries]]` entry per package.

```toml
[[binaries]]
package = "velnor-ci"
bin = "velnor-ci"
platforms = ["linux-x64", "linux-arm64", "macos-arm64"]
profile = "release-tools"
```

`profile` is optional and per binary. When it is unset, the build step runs `cargo build --release` and the pack step reads `target/<triple>/release/`. When it is set, the build step runs `cargo build --profile <profile>` instead of `--release`, and the pack step reads `target/<triple>/<profile>/` (under the root's own `target` folder for a root other than `.`). The checksum and publish jobs read only the packed archives, so they do not depend on the profile. A binary that sets no `profile` keeps `--release` even when another binary in the same file sets one.

Use it when a binary needs settings that `[profile.release]` must not carry, such as a size-tuned `opt-level`, thin LTO, `strip` and `panic = "abort"` kept for a few tools in a workspace whose other members stay at the release defaults. Declare the profile in the workspace root `Cargo.toml` with `inherits = "release"`.

The generator refuses a name that is empty, does not start with a letter or `_`, holds anything beyond letters, digits, `-` and `_`, or is one Cargo reserves or that collides with a `target` subfolder: `dev`, `test`, `bench`, `debug`, `doc`, `build-override`, `package`, `deps`, `examples`, `build` and `incremental`. The generator cannot read the manifest, because its view of the repository holds only the files it renders from, so it does not check that the profile is defined. A profile that is missing from the root `Cargo.toml` fails in the build step with Cargo's `profile ... is not defined` error, and the release job stops before it publishes.

## Release family tag

`family_tag` in `.velnor/release.toml` is opt-in. Without it the generated `release.yml` is unchanged. It sets one tag for every `[[binaries]]` entry, for example `family_tag = "tools-v{{ version }}"`. The pattern holds `{{ version }}` once and no `{{ package }}`. The version is the version of the binary packages, read from `cargo metadata` at the verified commit, and every binary must have the same one.

```toml
family_tag = "tools-v{{ version }}"

[[binaries]]
package = "app"
```

Each binary package must be listed in its `release-plz.toml` with `release = false`, so that the family tag is its only tag. Other packages keep their release-plz tags. The pattern must not collide with a release-plz tag pattern. The generator accepts any valid pattern with one `{{ version }}` and does not enforce a naming scheme. The group prefix of #1429 (`tools-v`, `runner-v`, `actions-v`, never a bare `v`) is a convention that the repository tag rulesets enforce, because the config has no field that says which prefix a project requires.

A `family` job binds the tag to the commit that the gate verified. It uses `velnor-ci release family-tag`, which needs a helper release of `0.1.83` or later. It creates the tag on that commit, and it refuses the release unless the tag resolves to that commit itself. A published release of the tag on an earlier ancestor commit releases nothing. The build jobs check out the tag, refuse any other commit, and share one `sums-family` and one `publish-family` job, so all binaries of a release share one tag, one commit, one `SHA256SUMS` and one release.

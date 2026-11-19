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

## Optional private dependencies

A project whose crates depend on private repositories names them in `.velnor/rust-ci.toml`. The table is off when absent, and then the generated workflow is unchanged.

```toml
[private_sources]
app_id = "vars.DEPS_APP_ID"
private_key = "secrets.DEPS_APP_PRIVATE_KEY"
repositories = ["owner/shared-lib"]
```

`velnor-gen generate` prints a note that suggests this table when `Cargo.lock` locks a dependency from a registry other than crates.io, from a git host that is not github.com, gitlab.com, codeberg.org or bitbucket.org, over ssh, or from a github.com owner that is the owner of this repository. It writes no secret, and it stays silent for crates.io and for public git dependencies of other owners. A git URL with credentials in it gets no suggestion: with `[private_sources]` configured, generation refuses it, so relock with a plain https URL.

`app_id` must be a `vars.` name and `private_key` a `secrets.` name. The variable holds the GitHub App's client ID, which the generated step passes to `actions/create-github-app-token` as `client-id`. `repositories` is a non-empty list of `owner/name` entries with one owner. Anything else is refused. The key needs a helper release of `0.1.86` or later.

Each job that compiles gets three steps after `jdx/mise-action` and before its first compiling cargo command and before any `[[test_setup]]` command, because a setup command is repository code. The steps run when any of those commands runs, not only the first one. The first, which runs on `merge_group` events only, runs `velnor-ci private-token-allowed`. It reads the queue entry from `github.event.merge_group.head_ref` (`gh-readonly-queue/<base>/pr-<number>-<sha>`, anchored on the last segment because the base branch may contain `/`), looks the pull request up with `GET /repos/{repo}/pulls/<number>` and the job's `GITHUB_TOKEN` (the job gets `pull-requests: read`), and writes `allowed=true` only when the pull request's head repository is this repository, its author's login does not end in `[bot]` (the same rule as for a pull request run, applied to every entry in the checkout) and its head sha is the queue entry's sha, and when the live queue ref of that entry still points at `GITHUB_SHA`, the commit this run checked out, so a queue that GitHub rebuilt after a fork entry left cannot authorize a run that still holds the fork's code. It authenticates every queued entry of the same base whose queue commit is in that checkout the same way, reading the checkout's commits beyond the base branch with one `GET /repos/{repo}/compare/<base>...<GITHUB_SHA>` (at most three pages of 100; a longer or unreadable answer fails closed), not one comparison per entry, so a long queue stays far below the 1,000 requests per hour that `GITHUB_TOKEN` allows. A malformed reference, a failed lookup, a fork, a sha mismatch or any other failure writes `allowed=false`, and the step still exits 0. The second creates a read-only token for the listed repositories with `actions/create-github-app-token`. It does not run for pull requests from forks, and it does not run for a pull request opened or pushed by a bot (an actor or pull request author whose login ends in `[bot]`, such as Dependabot or Renovate), because a dependency bot proposes code from a crate registry that nobody has reviewed yet. Such a pull request builds only from the caches, and the token comes at the merge queue, after a person has approved it. On `merge_group` it runs only when the first step wrote `allowed=true`, so a fork pull request that a maintainer enqueues gets no token. The third runs `velnor-ci fetch` with the token in its step environment only. `velnor-ci fetch` runs `cargo fetch --locked` for each Cargo root, gives the token to git through `GIT_CONFIG_*` variables of that command alone, and then revokes it with `DELETE /installation/token`. Before it runs cargo it reads the Cargo config files that cargo would read, with their `include` chains, to find the credential providers they declare; a chain of more than eight files is refused (the fetch runs nothing and the token is revoked) rather than read in part. `velnor-gen generate` itself refuses a Cargo config that has an `include`. The fetch sets `core.hooksPath=/dev/null`, `core.fsmonitor=false`, `core.sshCommand=ssh`, `protocol.ext.allow=never` and an empty `credential.helper` next to the token in its git settings, and `RUSTC`, `CARGO_BUILD_RUSTC` and the four rustc wrapper variables in its environment so that `build.rustc`, `build.rustc-wrapper` and `build.rustc-workspace-wrapper` of any config file cannot start a program, which outrank the config and hooks of a restored `~/.cargo/git` database, so a cached repository's hook cannot run with the token. When `cargo` is a rustup proxy, the fetch asks `rustup show active-toolchain` from `CARGO_HOME`, a directory the checkout does not control, and passes that name as `RUSTUP_TOOLCHAIN`, so a `rust-toolchain.toml` in the checkout cannot select a toolchain of its own; a name that is not a plain toolchain name is refused. The GitHub API client for the queue check and the revocation trusts only the certificates of the PEM bundle that `SSL_CERT_FILE` names, as git does, so a GitHub Enterprise Server behind a private CA works; when the variable is unset, or names a file that is unreadable or holds no certificate (a warning says so), it trusts the built-in Mozilla roots. It starts `cargo` and `git` by absolute path from the tool directories the runner installed, with an environment it builds itself, so a changed `mise.toml` cannot redirect the token. It refuses a `CARGO_HOME` outside those directories. It rewrites the forms of the server `VELNOR_SERVER_URL` names (`github.server_url`; `https://github.com` when unset, and only `https://<host>[:<port>]` is accepted) that a lock file records (`https://<host>/`, `http://<host>/`, `ssh://git@<host>/`, `ssh://<host>/` and `git@<host>:`; the three ssh forms name the host without the web port, so a server at `https://host:8443` has its ssh forms rewritten to `https://host:8443/`) to the authenticated URL, passes the runner's proxy variables (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` and their lower-case forms) through, runs each fetch in the directory of its manifest so that the `.cargo/config.toml` of a nested root applies, and passes the file `crates_mirror` wrote as `--config` when `VELNOR_CARGO_CONFIG` names it. `velnor-gen generate` refuses a `Cargo.lock` that locks a git dependency from an https URL with credentials in it. The policy job's `cargo-deny` runs after the same fetch phase. The token step keeps the action's post-job revocation, so a cancelled job still revokes the token; the revocation after a completed fetch makes that a warning. Without a token it runs `cargo fetch --locked --offline` and fails with a message that names the fork case when the caches lack a dependency.

With `[private_sources]` the `groups` shape builds and runs the tests in one job and does not upload a nextest archive: workflow artifacts can be downloaded by anyone who can read the repository, and the archive would hold the compiled private code.

Every later cargo command of the job runs with `--offline` and `CARGO_NET_OFFLINE=true`. The token is written to no file, to `$GITHUB_ENV` or to any git configuration.

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

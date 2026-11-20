# Generator configuration

The new Rust generator reads `.velnor/rust-ci.toml`.

The generator reads `.velnor/rust-ci.toml`, `.velnor/release.toml` and `.velnor/workflows.toml` only through Git's tracked view. A file that exists on disk but that Git does not track (or ignores) is refused with an error, not replaced by defaults: run `git add <path>` (`git add -f` for an ignored file), or delete the file.

## Fork pull requests

By default, a pull request from a fork does not select a self-hosted runner. When `runner` is not declared `hosted = true`, the generator renders `runs-on` of every job, including the `Required` gate, as an expression. It selects `runner` when the event is not a pull request or the pull request head repository is this repository, and `fork_runner` otherwise. No job is skipped for a fork, so the gate sees the real result of every job and fails when one fails. The one exception is a job that calls a reusable workflow with `uses`: it has no `runs-on`, and the callee may run on a self-hosted runner, so the generator adds the same condition to its `if` (joined with the job's own `if` in one expression) and a pull request from a fork skips the call. A deleted fork has no head repository, so it counts as a fork.

`fork_runner` is a GitHub-hosted label. It defaults to `ubuntu-latest`, to `ubuntu-24.04-arm` when `runner` is a Linux arm64 label and to `macos-15` when `runner` is an Apple silicon macOS label. The generator refuses a `fork_runner` that is not GitHub-hosted or that does not match the platform of `runner`. When the platform of `runner` is unknown, which is the case for a label such as `velnor-box`, the generator accepts only a `fork_runner` that is linux-x64, so `windows-latest`, `macos-15` and `ubuntu-24.04-arm` are refused until the platform is known. It also refuses a self-hosted `runner` label that holds anything beyond letters, digits, `-`, `_` and `.`, because the label is written into an expression.

The fence does not stop a fork pull request that edits its own copy of the workflow. The real boundary is outside this file: first, the repository setting Settings > Actions > General > Fork pull request workflows, set to "Require approval for all outside collaborators"; second, a self-hosted runner group whose repository access is limited to this repository, so no other repository's workflows can target its runners.

`hosted` declares what the generator cannot see: whether the runner behind `runner` is GitHub-hosted. The generator never decides it from the spelling of a label, because a self-hosted runner can be registered under any name, including `ubuntu-24.04`. Only `hosted = true` opens a runner to pull requests from forks; every other runner is self-hosted and fenced. The rules, in order:

1. `hosted = false` means self-hosted and fenced, even when the label looks GitHub-hosted.
2. `hosted = true` means GitHub-hosted, so `runs-on` is the plain label and a fork pull request runs on it.
3. With no `hosted` key, a label that is spelled exactly like a GitHub-hosted runner (`ubuntu-latest`, `ubuntu-24.04`, `ubuntu-26.04`, `ubuntu-slim`, `macos-15-large` and the rest of the fixed list, compared whole and without regard to case) makes the generator stop with `runner <label> looks GitHub-hosted; declare hosted = true (open to fork pull requests) or hosted = false (fenced) in rust-ci.toml`. A repository must say which it is.
4. With no `hosted` key, any other label (`velnor-box`, `ubuntu-26.04-scale-set`) is self-hosted and fenced, and `ubuntu-26.04-scale-set` is not matched by `ubuntu-26.04` as a prefix.

`hosted` sits next to `runner` in `.velnor/rust-ci.toml`, next to `runner` in a `[[test_groups]]` entry, and next to `runner` in a `[[workflow]]` or `[[workflow.job]]` of `.velnor/workflows.toml`. A job of a workflow whose `runner` is the same label inherits the workflow's `hosted`. One label has one value: the generator refuses a label that is declared `hosted = true` in one place and `hosted = false` in another, and a `crates_mirror.runner` counts as `hosted = false`. When `runner` is absent, the default `ubuntu-latest` counts as declared `hosted = true`; set `hosted = false` to fence it. In `rust-ci.toml` the key needs `velnor-ci` 0.1.99 or later, because `velnor-ci` reads that file at run time and an earlier release rejects the unknown key; the generator refuses an older pin, and `velnor-gen pin-helper 0.1.99` fixes it. `workflows.toml` is not read by `velnor-ci`, so its `hosted` key needs no particular release.

This is a breaking change of the configuration (issue 1376). A repository whose `runner` is a GitHub-hosted label, such as `runner = "ubuntu-24.04"` or `runner = "macos-15"`, must add `hosted = true` to the same table, and `velnor-gen generate` fails with the message above until it does. A custom label needs no change and stays fenced. The `hosted_runner` and `scale_set_runner` of `[[workflow.case]]` declare their own jobs: the `hosted_runner` jobs are `hosted = true` and the `scale_set_runner` jobs are `hosted = false`, and the two labels must differ. `fork_runner` is still checked by its spelling, because it must be a GitHub-hosted label.

```toml
runner = "ubuntu-26.04-scale-set"
hosted = false                  # optional: an undeclared custom label is self-hosted too
fork_runner = "ubuntu-24.04"
```

## Optional cargo fingerprint log

`cache_debug = true` in `.velnor/rust-ci.toml` makes every generated cargo step set `CARGO_LOG` from the repository variable `VELNOR_CACHE_DEBUG` and adds a step that uploads the log when that variable is `1`. It is `false` by default, and then the generated workflow reads no repository variable. A repository that sets it also declares `VELNOR_CACHE_DEBUG` to its workflow linter. The key needs a helper release that knows it (`0.1.82` or later), because older helpers reject unknown keys.

## Optional mbx remote cache

`[cache] mbx_remote = "github-actions"` in `.velnor/rust-ci.toml` adds `jdx/mr-boxington-action` (pinned by full SHA) to every generated job that builds with Cargo (a fmt, cargo-deny or cargo-machete lane, or a run of a prebuilt test archive, gets none), right after the tools install and before any repository code runs. It uses the action's `github` backend in `objects` mode, pinned to mbx `1.30.0` (the `mr-boxington` pin of `mise.toml`) with `version`. Each job's restores are scoped to its own generation (`cache-generation: v1-<job>`), so a job never warm-starts from another job's entry. The action saves after a successful push to the default branch and after a successful pull request from the same repository (`save-on-pull-request` compares the head repository with `github.repository`, and that save is scoped to the pull request's own ref, so main and other pull requests cannot restore it); a fork pull request, a protected branch or a manual run only restores. The step runs with `continue-on-error: true`, so a failed export or save never fails the Cargo lane. The objects cache holds only what mbx compiled, so the saving needs the build commands to run under `mbx`; the generated Cargo steps do not, so a repository that opts in must route its build through `mbx` (its own `[wrappers.cargo]`) for the cache to fill. The jobs get no write permission, no `id-token` and no secret, and the workflow sets no `MBX_REMOTE_*` variable. The action's post step still runs after the job's Cargo steps. It uses the runner's cache credential, not a repository token, and writes only this repository's own caches, from a push to the default branch or a same-repository pull request. The table is off by default, and then the output is unchanged. Allowed values: `"github-actions"`. The key needs velnor-ci 0.1.88 or later, because older helpers reject unknown keys, and `velnor-gen pin-helper 0.1.88` fixes an older pin. The `mbx-remote-cache` sample is pinned to the current release.

## Skip the push run after a merge queue run

`[ci] skip_push_after_merge_group = true` in `.velnor/rust-ci.toml` stops a push to the default branch from repeating checks that the merge queue already ran on the same commit. It is off by default, and then the generated workflow is byte-for-byte what it was without the key. It needs `velnor-ci` 0.1.103 or later, the first release whose `merge-group-passed` reads only the latest merge_group run on the commit and knows the job conditions of the skipped test jobs; an older release is refused at generation. The push is matched to the queue run by commit id, so the merge queue must keep the commit it tested (a squash or a fast-forward merge, as in this repository's ruleset). A queue that rebases or creates a merge commit pushes a different commit, and then the push never skips. A queue run that is still running counts when its `Required` job has already succeeded, and the job is found by the name in `required_check` (`velnor-ci` 0.1.97 or later; an earlier release waits for the whole queue run to complete).

```toml
[ci]
skip_push_after_merge_group = true
```

The merge queue fast-forwards the commit it tested, so the push to the default branch has the SHA of the `merge_group` run. On such a push, the plan job (or the single job, right after its plan) runs `velnor-ci merge-group-passed`. That step asks the GitHub API for runs of this same workflow file with `event=merge_group`, `head_sha=$GITHUB_SHA` and a completed, successful conclusion, and writes `skip=true` when one exists. It reads `github.token` and the plan job gains `actions: read`, nothing more.

When the flag is true, Format, Lint, Documentation, the single-crate checks, test setup, tests, doctests, `cargo deny` and `cargo machete` skip, and a test job that only runs archived tests (a test group in a workflow with a build job) does not start at all. A lane that planned everything again because its base expired runs its checks anyway, because the plan of the queue run did not cover the added packages. Builds that warm the caches and every cache save keep running, so the caches stay warm. When the workflow has a plan job (more than one job), every non-Rust `[[lint]]` job also waits for the plan job and skips all its steps on such a push; the job still succeeds, so the gate needs no change for it. A single-job workflow has no plan job to read, so each of its `[[lint]]` jobs runs `velnor-ci merge-group-passed` itself (after installing `velnor-ci`, with `actions: read`) and skips all its steps on such a push, as the plan-job form does. The `Required` gate then passes without test counts for this case only: the event is `push` and the plan job output `skip_push` is exactly `true`. Any other value, event or a failed job fails the gate as before.

The step fails safe. A non-push event, a missing variable, an API error, an unreadable response, a missing run, a run that failed, or a run that is still going without a succeeded `Required` job all write `skip=false`, and everything runs. The step is `continue-on-error`, so a crash also runs everything.

In the one-job shape the `rust` job itself is granted `actions: read` at job level, so the merge-group lookup can query the Actions API. On a public repository this exposes only workflow-run metadata that is already public; private-repository consumers should be aware of it.

## Pull request delta cache

A pull request from this repository restores a per-pull-request delta layer after the Target cache and saves it after the last compiling step, and a `rust-cache-trim.yml` workflow trims the deltas of finished pull requests. There is no key for it. It needs a locked `velnor-ci` 0.1.95 or later, because the helper has the `delta` commands. With an older helper (0.1.81 to 0.1.94) the generator silently renders no delta steps and no cleanup workflow; a helper from 0.1.95 to 0.1.101 renders the delta steps but no cleanup workflow, which needs 0.1.102 (the first release whose `cache trim-deltas` works after the pull request merges). It is not an error, so a repository pinned to an earlier release keeps generating. The layer is also left out when a custom `target-dir` is set, when `Cargo.lock` holds a source that is not proven public, or when it would not fit the budget (the generator reports what it left out).

## Optional crates mirror

The new generator accepts an explicit mirror in `.velnor/rust-ci.toml`. It is off when the table is absent. It applies only when the configured job runner exactly matches the mirror's runner label. Every emitted mirror step requires the actual runner context to report `runner.environment == 'self-hosted'` and excludes fork pull requests. Hosted runners do not use the mirror, regardless of their label; a missing or unrecognized runner environment also skips setup.

```toml
runner = "ubuntu-26.04-scale-set"

[crates_mirror]
runner = "ubuntu-26.04-scale-set"
registry = "sparse+https://crates-mirror.velnor-system.svc.cluster.local/index/"
cainfo = "/opt/velnor/crates-mirror/ca-bundle.pem"
```

The registry must use sparse HTTPS and end with `/`, and it holds no whitespace, control character or one of `'`, `"`, `$`, `` ` ``, `#`, `?`, `@` and `\`. A `runner` spelled like a GitHub-hosted label is refused, because the mirror is always self-hosted. The CA bundle must have an absolute path and already exist on the runner. The runner's image supplies it. The generator requires a pinned `velnor-ci` release of at least `0.1.93` when this table is present, including when the helper is built from source. The released helper configures Cargo before that bootstrap build.

Each opted-in job runs `velnor-ci crates-mirror` before Cargo or tool installation. It writes `$RUNNER_TEMP/velnor-cargo/config.toml` with `source.crates-io.replace-with = "velnor-crates-mirror"`, the sparse registry and `http.cainfo`. It leaves `$HOME/.cargo` and `$CARGO_HOME` untouched and refuses an existing job configuration that it does not own. Repeated setup replaces the owned configuration. Generation requires a valid HTTPS registry URL with a host. Cargo reports registry or CA failures as failures; it does not silently switch to public crates.io. When the runner's environment sets `VELNOR_CRATES_MIRROR=off`, the step prints that the mirror is disabled and returns before it reads the CA bundle, writes the configuration, installs the wrapper or exports anything, so Cargo keeps its normal sources. `velnor-local` sets it on every runner it starts, because neither the mirror nor its CA bundle exists on a developer machine. Any other non-empty value fails the step, so a typo cannot disable the mirror silently. Production runners never set it, and a missing CA bundle or an unreachable mirror still fails the job.

A Cargo wrapper on `PATH`, also exported as `CARGO`, passes the absolute configuration with Cargo's `--config` option. It runs a per-toolchain hard link of Cargo under a distinct executable name so Cargo preserves the wrapper in `CARGO` for build scripts and tests. When a hard link is unavailable, it installs a copy once. This keeps the mirror when a child command changes `CARGO_HOME` or runs from a temporary directory, and supports `cargo +toolchain`. Rustup selections from `rust-toolchain.toml`, directory overrides, or `+toolchain` remain in effect for child commands run from temporary directories. Calling a toolchain's Cargo by an unrelated absolute path bypasses `PATH` and `CARGO` and is outside this invocation contract. The wrapper counts its nested runs in `VELNOR_CARGO_DEPTH` and stops a chain after 64 of them (velnor-ci 0.1.106 or later), so a `cargo` on `PATH` that forwards back into the wrapper fails with a message instead of looping until the arguments overflow.

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

`velnor-gen generate` prints a note that suggests this table when `Cargo.lock` locks a git dependency from github.com that looks private: over ssh, or from a github.com owner that is the owner of this repository. It writes no secret, and it stays silent for crates.io and for public git dependencies of other owners. A source the token cannot authenticate (a registry other than crates.io, or a git host that is not github.com) gets a different note that says so and does not suggest the table; if your GitHub Enterprise Server hosts it, add the table anyway. A git URL with credentials in it gets no suggestion: with `[private_sources]` configured, generation refuses it, so relock with a plain https URL. With the table configured, generation prints a note when `Cargo.lock` locks a git dependency of the token's owner on github.com that `repositories` does not list (the token cannot read it, so a cold fetch fails if it is private), and when `repositories` lists a repository that no `Cargo.lock` locks (the token reads more than the build fetches).

`app_id` must be a `vars.` name and `private_key` a `secrets.` name. The variable holds the GitHub App's client ID, which the generated step passes to `actions/create-github-app-token` as `client-id`. `repositories` is a non-empty list of `owner/name` entries with one owner. Anything else is refused. The key needs a helper release of `0.1.88` or later.

Each job that compiles gets three steps after `jdx/mise-action` and before its first compiling cargo command and before any `[[test_setup]]` command, because a setup command is repository code. The steps run when any of those commands runs, not only the first one. The first, which runs on `merge_group` events only, runs `velnor-ci private-token-allowed`. It reads the queue entry from `github.event.merge_group.head_ref` (`gh-readonly-queue/<base>/pr-<number>-<sha>`, anchored on the last segment because the base branch may contain `/`), looks the pull request up with `GET /repos/{repo}/pulls/<number>` and the job's `GITHUB_TOKEN` (the job gets `pull-requests: read`), and writes `allowed=true` only when the pull request's head repository is this repository, its author's login does not end in `[bot]` (the same rule as for a pull request run, applied to every entry in the checkout) and its head sha is the queue entry's sha, and when the live queue ref of that entry still points at `GITHUB_SHA`, the commit this run checked out, so a queue that GitHub rebuilt after a fork entry left cannot authorize a run that still holds the fork's code. It authenticates every queued entry of the same base whose queue commit is in that checkout the same way, reading the checkout's commits beyond the base branch with one `GET /repos/{repo}/compare/<base>...<GITHUB_SHA>` (at most three pages of 100; a longer or unreadable answer fails closed), not one comparison per entry, so a long queue stays far below the 1,000 requests per hour that `GITHUB_TOKEN` allows. A malformed reference, a failed lookup, a fork, a sha mismatch or any other failure writes `allowed=false`, and the step still exits 0. The second creates a read-only token for the listed repositories with `actions/create-github-app-token`. It does not run for pull requests from forks, and it does not run for a pull request opened or pushed by a bot (an actor or pull request author whose login ends in `[bot]`, such as Dependabot or Renovate), because a dependency bot proposes code from a crate registry that nobody has reviewed yet. Such a pull request builds only from the caches, and the token comes at the merge queue, after a person has approved it. On `merge_group` it runs only when the first step wrote `allowed=true`, so a fork pull request that a maintainer enqueues gets no token. The third runs `velnor-ci fetch` with the token in its step environment only. `velnor-ci fetch` runs `cargo fetch --locked` for each Cargo root, gives the token to git through `GIT_CONFIG_*` variables of that command alone, and then revokes it with `DELETE /installation/token`. Before it runs cargo it reads the Cargo config files that cargo would read, with their `include` chains, to find the credential providers they declare; a chain of more than eight files is refused (the fetch runs nothing and the token is revoked) rather than read in part. `velnor-gen generate` itself refuses a Cargo config that has an `include`. The fetch sets `core.hooksPath=/dev/null`, `core.fsmonitor=false`, `core.sshCommand=ssh`, `protocol.ext.allow=never` and an empty `credential.helper` next to the token in its git settings, and `RUSTC`, `CARGO_BUILD_RUSTC` and the four rustc wrapper variables in its environment so that `build.rustc`, `build.rustc-wrapper` and `build.rustc-workspace-wrapper` of any config file cannot start a program, which outrank the config and hooks of a restored `~/.cargo/git` database, so a cached repository's hook cannot run with the token. When `cargo` is a rustup proxy, the fetch asks `rustup show active-toolchain` from `CARGO_HOME`, a directory the checkout does not control, and passes that name as `RUSTUP_TOOLCHAIN`, so a `rust-toolchain.toml` in the checkout cannot select a toolchain of its own; a name that is not a plain toolchain name is refused. The GitHub API client for the queue check and the revocation trusts only the certificates of the PEM bundle that `SSL_CERT_FILE` names, as git does, so a GitHub Enterprise Server behind a private CA works; when the variable is unset, or names a file that is unreadable or holds no certificate (a warning says so), it trusts the built-in Mozilla roots. It starts `cargo` and `git` by absolute path from the tool directories the runner installed, with an environment it builds itself, so a changed `mise.toml` cannot redirect the token. It refuses a `CARGO_HOME` outside those directories. It rewrites the forms of the server `VELNOR_SERVER_URL` names (`github.server_url`; `https://github.com` when unset, and only `https://<host>[:<port>]` is accepted) that a lock file records (`https://<host>/`, `http://<host>/`, `ssh://git@<host>/`, `ssh://<host>/` and `git@<host>:`; the three ssh forms name the host without the web port, so a server at `https://host:8443` has its ssh forms rewritten to `https://host:8443/`) to the authenticated URL, passes the runner's proxy variables (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` and their lower-case forms) through, runs each fetch in the directory of its manifest so that the `.cargo/config.toml` of a nested root applies, and passes the file `crates_mirror` wrote as `--config` when `VELNOR_CARGO_CONFIG` names it. `velnor-gen generate` refuses a `Cargo.lock` that locks a git dependency from an https URL with credentials in it. The policy job's `cargo-deny` runs after the same fetch phase. The token step keeps the action's post-job revocation, so a cancelled job still revokes the token; the revocation after a completed fetch makes that a warning. Without a token it runs `cargo fetch --locked --offline` and fails with a message that names the fork case when the caches lack a dependency.

With `[private_sources]` the `groups` shape builds and runs the tests in one job and does not upload a nextest archive: workflow artifacts can be downloaded by anyone who can read the repository, and the archive would hold the compiled private code.

Every later cargo command of the job runs with `--offline` and `CARGO_NET_OFFLINE=true`. The token is written to no file, to `$GITHUB_ENV` or to any git configuration.

## Workspace groups

A repository with many small Cargo workspaces can check several of them in one job set, when the project asks for it. A group names the workspaces that share one job set; a workspace that no group names keeps its own jobs.

```toml
[[workspace_groups]]
name = "tools"
roots = ["tools/a", "tools/b", "tools/c"]
```

`name` is the group's job name and its cache name; it uses lowercase letters, digits and `_`, and it must not be the name of a Cargo root of the repository. `roots` are the folders of the roots' `Cargo.toml` files, or `.` for the repository root. Every root exists in the repository and belongs to at most one group, and a group has at least two roots.

A group gets one set of jobs, named after the group. The job runs each cargo command once per root, one after another, with `--manifest-path` naming that root's manifest, from the repository root. All roots of a group share one build folder, `target-velnor/<name>`, so a crate that several of them use at the same version and features is compiled once. The group has one sources cache and one target cache, and the target cache key hashes the lock file of every root of the group. A root with nothing selected is skipped inside the job; the job itself runs when any root of the group is selected.

A root of a group cannot have `[[test_groups]]` or `[[test_lane]]` entries; generation fails when one names it.

The key needs a helper release that writes the plan output of every group (`0.1.96` or later); generation fails with an older pin.

## Optional shared MBX store

`mbx_shared_store = true` in `.velnor/rust-ci.toml` makes every job run `velnor-ci mbx-store` right after the helper is installed. It needs `velnor-ci` 0.1.85 or later and is off by default.

The runner host may set `VELNOR_MBX_SHARED_DIR` in the runner pod. The workflow `env` context cannot see the runner process environment, so the helper decides at run time. On a trusted event (a push or schedule on the default branch), when the directory is absolute, exists, is not a symlink and contains no symlink, and is writable, the helper appends `MBX_CACHE_DIR=<dir>` and a pod-private `MBX_SHIMS_DIR=$RUNNER_TEMP/velnor-mbx-shims` to `$GITHUB_ENV`. A pull request, a fork, a `merge_group` run, or any other event never gets the shared directory. A directory that fails a check produces a `::warning::` and the job keeps MBX's own per-job store. Without the variable the step changes nothing.

With `velnor-ci` 0.1.86 or later in the lock, `mbx_shared_store = true` also renders a `velnor-ci mbx-seed` step right after `mbx-store`, with `if:` set to pull_request or merge_group. The runner host may mount the trusted directory read-only and set `VELNOR_MBX_SEED_DIR` to it. When that directory is absolute, exists, is not a symlink, contains no symlink on its path, is owned by the runner user or root and is not world-writable, the helper copies the missing files of `actions/cas`, `actions/executable-identities`, `actions/task-manifests` and `actions/action-results` into the private store `$RUNNER_TEMP/velnor/mbx`, in parallel, and appends `MBX_CACHE_DIR=<private store>` to `$GITHUB_ENV`. It logs the file count, bytes and seconds. It never writes to the seed and never follows a symlink inside it. Each copied file keeps only its read, write and execute bits: a setuid, setgid or sticky bit in the seed is dropped. Without the variable, with an empty seed, or when a check fails (with a `::warning::`), it changes nothing and MBX builds cold. With an older helper the step is not rendered and pull requests stay cold.

`docs = "post_merge"` in `.velnor/rust-ci.toml` runs the `Documentation` step (`cargo doc`) only on `push`, `schedule` and `workflow_dispatch`, so a pull request and a `merge_group` run skip it. The default, `"always"`, keeps the step on every event. The key needs `velnor-ci` 0.1.97 or later in the lock, because an older helper refuses the unknown key when it reads the configuration, and generation names that release. Before the merge the plan lists documentation as left out (`docs = "post_merge" runs it only after the merge`), and the setting is reported as a check that does less, so a change that turns it on is named in review. A deferred step is never reused from a successful `merge_group` run (`skip_push_after_merge_group`), so the run after the merge always does it. mbx caches rustdoc, but it keys each crate's action on the whole workspace source tree, so any source change misses every crate and a pull request recompiles all the docs; the doc check then moves to the run after the merge.

`check_alone = "post_merge"` does the same for the per-crate `Check <crate> alone` steps. They compile each crate with only its own feature set, so their dependency artifacts differ from the workspace lint and build steps and a pull request rarely finds them in the mbx cache. They keep their value as the check that a crate declares every feature it needs, and the run after the merge keeps that check. The default is `"always"`. The key needs `velnor-ci` 0.1.97 or later in the lock, and the same weakening and reuse rules apply.

## Test setup steps

A test that needs a fixture, such as a built example executable or a provisioned service, declares one step per fixture. The generator runs each step before the first test step of every job of that root, in the order written. Each step is one command, because a generated `run:` value is one command with no shell logic; the logic lives in a script the repository commits. The script exports variables to later steps through `$GITHUB_ENV`.

```toml
[[test_setup]]
root = "."
name = "Prepare the fixtures"
run = "bash scripts/prepare-fixtures.sh"
```

`root` is the folder of the Cargo root's `Cargo.toml`, or `.` for the repository root, and it must name a root of this repository. The step runs in that folder. It follows the same gate as the test steps, so a run that selects no tests skips it. A job that runs no tests has none. With `shape = "groups"` the test group job installs only cargo-nextest, so a step there must not need Rust. A `run` value with an expression, a line break or any shell syntax is refused.

## Feature test lanes

A package may have tests that exist only under a Cargo feature. A `[[test_lane]]` entry adds one nextest step to the root's test job. The step runs the selected tests again with the features on.

```toml
[[test_lane]]
root = "crates/velnor-runner-*"
features = ["local-mode"]
packages = ["velnor-runner-host"]
filter = "package(velnor-runner-host) and (test(~local::) or binary(local_host_public) or binary(local_pair_public))"
```

`root` is the folder of the Cargo root's `Cargo.toml`, or `.` for the repository root. It must name a root of this repository that runs nextest tests. `features` is a non-empty list of distinct feature names, and each name uses letters, digits and `_-+./`. The step is named `Test (features: local-mode)` and runs `cargo-nextest nextest run --workspace --locked --features 'local-mode'` with the same selection as the root's default test step, narrowed by `filter`. It runs after the default test step and reports no test counts, so the default step stays the single source of the job's test counts. With `shape = "groups"` the step runs in the build job, which has Rust, because the archive holds only the default-feature build.

`packages` names the packages of the root whose tests the lane selects. It is required with `filter` and optional without it. The plan narrows a pull request to the changed packages, so a lane whose filter matches nothing there is not broken. When the plan selects none of `packages`, and not the whole workspace, the lane step is skipped and a step named `Skip test lane (features: ...)` prints `skipped: no selected packages`. Otherwise the lane runs with `--no-tests=fail`, so a filter that matches nothing fails the job instead of passing with zero tests. A lane without `filter` runs whenever the plan selects the root.

`filter` is optional. It is a nextest filterset that the generator joins to the plan's selection with `and`, so a lane never runs a test the plan did not select. Without it the lane re-runs the whole selected set with the features. Every generated `run:` value is one command with no shell syntax, so the filterset is one quoted argument and must not contain `|`, `&`, `;`, `<`, a quote, `$`, a backtick, a backslash, a line break or one of the words `if`, `for`, `while`, `case` and `coproc`. Write the nextest operators as words: `or` for `|`, `and` for `&` and `not` for `!`. Binaries that run with `cargo test` because their harness is off stay out of the lane, as they stay out of the default step.

`lint` and `doc` are optional booleans, `false` by default. A lane without them renders exactly as before. `lint = true` adds a clippy step named `Lint (features: local-mode)` and `doc = true` adds a rustdoc step named `Documentation (features: local-mode)`. Each runs the root's default command with `--features '<features>'` added, so clippy keeps `--workspace --all-targets --locked ... -- -D warnings` and rustdoc keeps `--workspace --no-deps --locked` with `-D warnings` for rustdoc. The steps follow the lane's test step in the same job, in the same folder and with the same target directory, so the build cache is shared. They are steps of a job that the `required` job already needs, so a failure there fails the gate. Format checks need no features and stay in the default lint step. Lanes of one root that name the same features, in any order, share one lint step and one rustdoc step, because those commands cover the whole workspace and ignore `filter`. The step appears after the first lane that sets `lint` or `doc`.

```toml
[[test_lane]]
root = "crates/velnor-runner-*"
features = ["local-mode"]
packages = ["velnor-runner-host"]
filter = "package(velnor-runner-host) and (test(~local::) or binary(local_host_public) or binary(local_pair_public))"
lint = true
doc = true
```

A lane with a `filter` gets a second step, `Test lane guard (features: ...)`, right after the lane step. It runs `velnor-ci lane-guard --features '...' --filter '...'`, with one `--except-binary` for each `harness = false` binary that the root runs with `cargo test`, and it runs under the same condition as the lane step. The guard asks cargo-nextest to list the workspace with and without the features and under the filter. It fails the job when a test exists only with the features, is not ignored, and falls outside `filter`, and it names each such test with its binary id. A listing error also fails the guard. A lane without a `filter` runs every selected test and has no guard.

The pinned velnor-ci reads `.velnor/rust-ci.toml` at run time and rejects a key it does not know, so any `[[test_lane]]` needs velnor-ci 0.1.93 or later, the first release that accepts the key and runs `lane-guard`. `velnor-gen generate` refuses an older pin when the file has a lane, filtered or not.

## Unused dependencies

The policy lane runs `cargo-machete` once from the repository root when the repository's `mise.toml` pins `cargo-machete`. It needs no `rust-ci.toml` entry. The pin is the opt-in, so the lane installs the tool through the same pinned mise path as every other tool. `cargo machete` reads every `Cargo.toml` below the root, not only the workspace members, and it skips what `.gitignore` and `.ignore` exclude. A repository that tracks manifests which are not crates (test samples, fixtures) lists them in a root `.ignore`; velnor-new does so for `generator/samples/velnor-new/view/crates/`.
## Optional speed budget

The new generator accepts a speed budget in `.velnor/rust-ci.toml`. It is off when the table is absent, and every key is off when it is `0`. The generator emits a `rust-budget` job when at least one key is above `0`. The job runs on every push to the default branch, except a push that `skip_push_after_merge_group` skipped because the merge queue had already passed its commit (the plan's `skip_push` output is `true`), and also on the scheduled full run when `full_run` schedules one (a scheduled run is a `full` selection and is measured against earlier scheduled runs, a push against earlier pushes of its own selection). It never runs on a pull request or a merge queue run. It runs after the required check has ended, also when a job before it was skipped, and only when the required check succeeded. The required check does not wait for it, so its failure never fails the required check. A run that failed the budget is not a sample for the next run, so a breach clears with the next run that is fast again.

```toml
[budget]
pr_wall_seconds = 0
main_wall_seconds = 0
runner_seconds = 0
setup_seconds = 0
```

`pr_wall_seconds` bounds the wall time of a pull request run, `main_wall_seconds` the wall time of a default branch run, `runner_seconds` the summed runner time of a run, and `setup_seconds` the time from the start of a job to its first cargo command. The job runs `velnor-ci bench budget --last 10`: it measures the run that executes it so far, collects the successful first attempts of the generated workflow before it for each kind of event (ten runs in the window with the running one), compares the median of that window and the running run with the budget, prints the three jobs whose duration is furthest above their median, and fails when a measurement is above its budget or when the runs do not have a measurement the budget needs. The generator requires a pinned `velnor-ci` release of at least `0.1.92` when a key of the table is above `0` (a table of zeros needs none).

### Limits per selection

A push to the default branch does very different work depending on what the plan selected: a push that changes no package runs nothing but the gate and takes under a minute, and a push that changes a core crate tests every crate that depends on it and takes ten times as long. One median of all of them flaps. The budget therefore measures each kind of run against the runs of the same selection:

| Selection | The plan wrote | Table |
|---|---|---|
| full | `rust_<root>_mode=all` for some root | `[budget.full]` |
| affected | `some` for some root and `all` for none | `[budget.affected]` |
| none | `none` for every root | `[budget.none]` |

```toml
[budget]
main_wall_seconds = 900        # the fallback of every selection that has no key of its own

[budget.full]
main_wall_seconds = 1200
runner_seconds = 1500

[budget.affected]
main_wall_seconds = 800

[budget.none]
main_wall_seconds = 90
runner_seconds = 120
```

A mode table takes the same four keys as `[budget]`. A key the table leaves out takes the plain key of `[budget]`, so the plain keys keep working as the limit of every selection. A key set to `0` in a mode table switches that check off for that selection only. A table holding a limit enables the budget even when every plain key is `0`.

The run that executes the budget job is judged against the earlier successful first attempts of its own selection, and against nothing else: the window of ten holds only runs of that selection, so a full run after nothing but affected runs has only itself in its window and passes when it is inside its own limit. The report prints one line of medians for each selection.

The selection is recorded in the title of the budget job, because the REST API returns the title of a job of a past run and returns no step output. The generated job is titled `Speed budget (full)`, `Speed budget (affected)`, `Speed budget (none)` or `Speed budget (unknown)`, from the `rust_<root>_mode` outputs of the job that runs `velnor-ci plan`; the budget job therefore needs that job. A run whose budget job has no title of this form, such as every run made before the workflow was regenerated, belongs to an unmeasured group that is judged by the plain keys only and is never mixed into the window of a known selection. The generator writes the title, and the per-mode tables, only for a pinned `velnor-ci` release of at least `0.1.104`, and refuses a mode table under an older pin.

The check fails closed. A budget for a kind of run there is none of is a failure, not a pass, and so is a budget whose runs do not carry a measurement it needs. The report is printed before the check fails, so that the breach is read next to the medians it broke.

A budget is a number the project measures, not a number it guesses: set every key to the median of the relevant runs plus a quarter, so that a run of ordinary noise stays inside the budget. The job prints the wall and runner seconds of the run so far next to the budget, so that a breach can be read next to the numbers it broke.

## Optional sparse checkout

`sparse_checkout = true` in `.velnor/rust-ci.toml` makes the checkout step of each Rust lane job list only what that lane reads: its root and its members, the roots it depends on, the lockfiles and tool files at the repository top, `.cargo/`, `.config/`, `.velnor/`, and every path of the inputs lock and of declared `[[inputs]]`. It is `false` by default, and then the generated workflow is unchanged. The plan, policy and audit-plan jobs keep the full tree. The lane keeps a full checkout when a root manifest sits at the repository top, when the inputs lock has a repository-wide read, or when `[[test_setup]]` is set, because those reads are unknown. A test that reads a tracked file outside the list fails with a missing file; add that path under `[[inputs]]`. From `velnor-ci` 0.1.103, the `velnor-ci report` step of the failed lane finds each such path in the test output (a "No such file or directory" line that names a tracked file the checkout skipped) and prints the `[[inputs]]` entry to add in the lane's section of the job summary, which the `Required` job repeats. With the setting off, `velnor-gen generate` prints one note when the tracked files outside the paths the Rust jobs read add up to more than 100 MiB.

## Run selection

These keys choose which jobs a run starts. The defaults are in the table in [`docs/rust-ci.md`](../docs/rust-ci.md).

- `shape` is `"auto"` (default), `"one-job"`, `"lint-test"` or `"groups"`. `auto` picks the shape from `.velnor/rust-ci-timings.json`; any other value is fixed. When `auto` has no build times for a root, `velnor-gen generate` and `check` print a note that names the one-job shape it picked, the missing timings and how to choose a shape or record times. A root with a `[[test_groups]]` entry always uses the `groups` layout.
- `on_main` is `"affected"` (default) or `"everything"`. It sets what a push to the default branch and a merge queue run select.
- `full_run` is `"nightly"` (default), `"hourly"`, `"weekly"`, `"off"` or a five-field cron expression. It sets the schedule of the run that selects everything.
- `skip_draft_pull_requests` is `false` (default). With `true`, a draft pull request runs nothing. It cannot be combined with `pr_ci = "label"`.
- `timeout_minutes` is the time limit of each job, 1 to 360, `30` by default.
- `runner` is the runner label of every job, `"ubuntu-latest"` by default. See Fork pull requests for `hosted` and `fork_runner`.
- `required_check` is the name of the one required check, `"Required"` by default. The name must not be blank or contain a control character. Any other name needs velnor-ci `0.1.97` or later, because the helper finds the check by that name.
- `default_branch` is `"main"` by default. It must equal the repository's default branch, or the run fails.

The shapes, and the jobs each one starts:

| `shape` | Jobs | Chosen when |
|---|---|---|
| `one-job` | `rust` | `auto` and `.velnor/rust-ci-timings.json` has no entry for the root, or the shorter of its lint build and test build takes 5 s or less |
| `lint-test` | `rust-lint`, `rust-test` | `auto` and the shorter build takes more than 5 s |
| `groups` | `rust-lint`, `rust-test` (builds the tests once), `rust-test-default`, `rust-test-<name>` | `shape = "groups"`, or the root has a `[[test_groups]]` entry |

## Files and packages

- `inputs` is a list of `[[inputs]]` entries. Each has a `glob`, a path glob, and `packages`, a non-empty list of package names. The files the glob matches belong to those packages. For a file that no package owns, `velnor-ci plan --explain` says what to add.

  ```toml
  [[inputs]]
  glob = "assets/**"
  packages = ["app"]
  ```

- `non_inputs` is a list of globs whose files select nothing. A glob may not overlap an `inputs` glob; the generator refuses that.
- `exclude` is a list of globs. A `Cargo.toml` that a glob matches is not a Cargo root. Each glob must match a tracked `Cargo.toml`, or the config is refused. A trailing `/` is removed.
  An excluded nested workspace keeps its `Cargo.lock` out of the sources cache key and the prefetch, so no job can fetch its packages. Generation then checks every job for cargo runs against it:
  - A declared `run` step in `.velnor/workflows.toml` that runs cargo against an excluded root's directory (`--manifest-path <dir>/Cargo.toml`, `cd <dir>` before cargo, or `working_directory = "<dir>"`) is refused. The message names the `Cargo.lock` and the fix: remove the root from `exclude`, or stop running cargo against it in CI.
  - A mise task in `mise.toml` or a hand-written workflow under `.github/workflows/` may run cargo against an excluded root. The text is read without a shell or YAML parser, and the job that runs it is not known, so generation prints a note with the same fix instead of refusing.
  - An excluded root that no step names produces no note.
- `default_ignores` is `true` by default: documentation, licence files and root-level `*.md` files select nothing. `false` lets them select.

## Cache and debug settings

- `cache_budget_bytes` is the cache budget in bytes. It is `8000000000` (8 GB) by default, and a larger value is refused. `Cache maintenance` keeps the repository under it.
- `cache_check_blocks` is `false` by default. `true` makes the `Cache` check part of `Required`.
- `ci_debuginfo` is `"line-tables-only"` (default), `"full"` or `"none"`. It sets the debug info of the CI build profile.
- `nextest_harnesses` is a list of `"package:target"` entries for `harness = false` test targets that nextest can run. Other such targets run in a direct step.

## Helper from this repository

`helper = { path = "tools/velnor-ci" }` names a folder of this repository that holds the source of `velnor-ci`. The generated workflow then builds the helper from that folder instead of using a release. A trailing `/` is removed. The path must be a valid repository path.

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
- `paths` (globs, as in `inputs`) and `events` (`pull_request`, `push`, `merge_group`, `schedule`, `workflow_dispatch`) are optional. A group with neither key runs whenever its root is selected. A group with either key is decided at run time by `velnor-ci plan`, which writes one output `rust_<root>_group_<name>` (`run` or `skip`) that is the job's `if`. The group runs when its root has a selected package and one of these holds, in this order: the event is a push to the default branch (every group runs, but only where the default lanes select the root); the event is listed in `events`; the event is a merge group and the group sets no `paths` (`events` never skip a group on a merge group); a changed path matches `paths`; every package is selected. Otherwise the job is skipped, and the `Required` check expects it to be skipped. On a merge group, `paths` are matched against the merge group's own base to head diff, the same diff the default lanes select by; when that diff cannot be computed, every package is selected and every group runs. `velnor-ci plan --explain` and the plan summary say why each group ran or was skipped. On a push to the default branch the groups follow exactly the same selection as the default lanes, neither stricter nor looser: with `on_main = "affected"` a docs-only push selects no package, so the lanes and every group skip; with `on_main = "everything"` the lanes and every group run. The default job still leaves out the group's tests, so tests of a skipped group do not run in that workflow run.
- `hosted = true` or `hosted = false` declares the group's `runner` (see Fork pull requests). It needs a `runner` in the same group, and, like the top-level key, `velnor-ci` 0.1.99.

A repository with any `[[test_groups]]` splits each root that has a group into the archive build job and the test jobs, whatever `shape` is: the group jobs run from the archive of the build job (`shape = "groups"`), and lint and doctests stay on the build job. The default job `rust-test-default` leaves out every group's tests with `and not (<filter>)`, and every group job is a need of the `Required` check. Two groups with one name in one root are refused. A root with two or more groups gets a step `velnor-ci test-groups check` in its build job, after the archive is built and before it is uploaded: it lists the workspace once per group with `cargo-nextest`, under the group's filter, and fails naming every test that two groups select (ignored tests are exempt), so the overlap is found at run time, not guessed from the filters. The key needs `velnor-ci` 0.1.90 or later, because the helper reads the same file.

`velnor-gen suggest-groups` reads `.velnor/rust-ci-timings.json` and prints a `[[test_groups]]` block for every test binary that takes more than a third of its root's total test-binary time, slowest first and by name on a tie. Each block has a name made from the binary id, `filter = "binary_id(=<id>)"` and, when the repository has several Cargo roots, `root`. A binary that a group of the same root already filters by is left out. The command writes no file, needs no pinned `velnor-ci`, and prints nothing on standard output when no binary is that slow.

## Non-Rust lint jobs

A `[[lint]]` entry adds one job to the generated workflow for a check that is not a Cargo lane. Each entry becomes the job `lint-<id>`. The job checks out the repository at depth 1, installs only the tools it names, and runs commands. It has `contents: read` and no other permission (plus `actions: read` in a single-job workflow with `skip_push_after_merge_group`), no `needs` (it starts with the plan job, not after it), and the same fork fence as every other job: a pull request from a fork runs it on `fork_runner`. A generated `run:` value is one command with no shell logic, and a lint job adds no action to the fixed list: every tool it runs comes from `mise.toml`, checksummed in `mise.lock`. A lint job runs on every ready pull request, also when `pr_ci` is `label`.

```toml
[[lint]]
kind = "jactionlint"
config = ".github/jactionlint.yaml"

[[lint]]
kind = "alint"
config = ".alint.yml"
probe = "scripts/alint-line-cap-probe.sh"

[[lint]]
kind = "mise-task"
id = "generator-samples"
name = "Generator samples"
task = "lint-generator-samples"
tools = ["jactionlint"]
timeout_minutes = 5
```

| Key | Kinds | Meaning |
|---|---|---|
| `kind` | all | `jactionlint`, `alint` or `mise-task`. |
| `id` | all | Job id suffix. Defaults to the kind, or to the task name for `mise-task`. Two entries cannot share it. |
| `name` | all | Job name. Defaults to `Jactionlint`, `Alint` or `Mise task <task>`. |
| `timeout_minutes` | all | 1 to 360, default 10. |
| `config` | `jactionlint`, `alint` | Configuration file. Required for `alint`. |
| `probe` | `alint` | A script run with `bash` after alint, for rules alint cannot express. |
| `task` | `mise-task` | The mise task, run as `mise run --force --skip-tools <task>`, so a task with fresh `sources` and `outputs` still runs. It must be a key of `[tasks]` in `mise.toml` or a file in a mise task folder (`mise-tasks`, `.mise-tasks`, `mise/tasks`, `.mise/tasks`, `.config/mise/tasks`); generation fails otherwise. |
| `tools` | `mise-task` | Tools from `mise.toml` to install before the task. The task runs with `--skip-tools`, so a tool it needs and `tools` lacks fails the job instead of being installed unpinned. Generation fails when the task, or a task it reaches through `depends`, declares a tool in its own `tools` table that this list does not name, because the runner's own binary of that name would run instead. The tools a task script calls without declaring them, and the headers of task files, are not read. |

`jactionlint` is the version pinned in `mise.toml` (and its checksum in `mise.lock`); generation fails when `mise.toml` does not pin it. It checks the repository with `--strict-exit --no-online`. It runs ShellCheck when `mise.toml` pins `shellcheck`, and turns it off otherwise. When the repository has a workflow step that uses `actions/github-script`, jactionlint checks its script only with `node` on `PATH`, so generation fails unless `mise.toml` pins `node`; a pinned `node` is installed next to `jactionlint`. A path or task name that is not made of letters, digits and `_-./:@`, or that starts with `-`, is refused, because it is written into a command.

`alint` is the version pinned in `mise.toml` (for example `"github:asamarts/alint" = "0.22.1"`) with the archive digest of every platform in `mise.lock`, so the digest is the repository's own and no upstream installer runs. It runs `alint --format github --config <config> --fail-on-warning check .`, so a warning fails the job. The file named by `config` or `probe` is not checked at generation time; a missing file fails the job, and so the `Required` check.

Every tool a lint entry needs is checked at generation. The tool must be pinned in `mise.toml` by its basename (`jactionlint`) or in full (`aqua:owner/tool/jactionlint`); a basename that matches two configured tools is refused until the entry names one in full. At least one of its requests must apply to the runner's operating system (the `os` key of the pin), and `mise.lock` must hold an entry that covers every version requested for that system. A `shellcheck` pin that is filtered out for the runner is treated as absent.

### One required check

The `Required` job (named by `required_check`) lists every lint job in `needs` and in its gate spec with no condition, next to the Rust lanes. A lint job that failed, was cancelled, was skipped or is absent from `needs` fails the gate, and a job in `needs` that the spec does not name fails it too. A ruleset therefore requires one check name for the Rust lanes and the lint lane together.

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

## Optional family tag for binary releases

`family_tag` in `.velnor/release.toml` gives every `[[binaries]]` entry of a release one tag, such as `tools-v0.1.80`, bound to the verified source commit. Its value holds one `{{ version }}` and no `{{ package }}`, and the repository needs at least one `[[binaries]]` entry. Without it, each binary keeps its own release-plz tag.

- A `family` job reads the version of every binary from `cargo metadata` at the verified commit, and all must agree. It runs `velnor-ci release family-tag`, which creates the tag on the verified commit, refuses unless the tag resolves to that commit, and opens one draft release. A run at a version that already has a release does nothing.
- Build jobs check out `refs/tags/<family tag>` and refuse any commit other than the verified one. The `sums-family` and `publish-family` jobs publish the assets of every binary under the one tag.
- The generator refuses a binary that release-plz still releases under its own tag, and a helper older than `velnor-ci` 0.1.85.
- Binary packages are `release = false` in release-plz, so their version is bumped by hand or by a pull request.

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

## Optional release asset names

A `[[binaries]]` entry in `.velnor/release.toml` publishes `<bin>-<target-triple>.tar.gz` by default, for example `app-x86_64-unknown-linux-musl.tar.gz`. `asset_name` replaces the name without the `.tar.gz` suffix, so a consumer that expects an exact file name, such as a package manager recipe, keeps it. The key is optional; without it the default name above applies.

```toml
[[binaries]]
package = "app"
platforms = ["linux-x64", "macos-arm64"]
asset_name = "{{ bin }}-{{ version }}-{{ target }}"
```

The placeholders are `{{ bin }}`, `{{ platform }}` (`linux-x64`), `{{ target }}` (the Rust target triple the job builds, such as `x86_64-unknown-linux-musl`) and `{{ version }}`. Write them exactly as shown, with one space inside the braces. The template must contain `{{ platform }}` or `{{ target }}`, so each platform gets its own file, may use only letters, digits, `.`, `_` and `-` besides the placeholders, and must start with a letter or digit. Anything else is refused at generation.

`{{ version }}` is the version in the released tag. The build, `SHA256SUMS` and publish jobs each read it from the tag with a step that sets `VERSION` through `$GITHUB_ENV`, and the build job packs into a `stage` folder that it uploads. The tag name must end in the version, as the default `{{ package }}-v{{ version }}` does. A template without `{{ version }}` adds no step and no folder.

`asset_name` applies to `format = "raw"` too: the template names the executable, and the `.sha256` and `.provenance.json` files take the same name. With `bins` on a raw binary, each extra binary is named by the same template with its own `{{ bin }}`, so the template must contain `{{ bin }}` or the assets would collide, and a template without it is refused. In an archive, `{{ bin }}` is the main binary.

### Raw binary keys

Three keys of a `[[binaries]]` entry add what the old `release-rust-ci-tools.yml` (now `release.yml`) did, in either format. All are off by default, and then the generated workflow is byte-identical to one generated without them.

```toml
[[binaries]]
package = "velnor-ci"
format = "raw"
target = "musl"
smoke = true
commit_file = true
```

- `target = "musl"` builds `<arch>-unknown-linux-musl` for the linux platforms of a raw binary, with the musl C compiler installed, so the executable is static. `macos-arm64` keeps `aarch64-apple-darwin`. The default for raw stays `"gnu"`. `target = "musl"` is refused without a linux platform. The archive format accepts `target` too and defaults to musl.
- `smoke = true` adds two steps to each build job after packing, for the main binary of each platform: `./<asset> --version`, then `./<asset> check-tag "$TAG"`, where `TAG` is the tag the release records for the package. The binary must implement `check-tag <tag>`, which fails when the tag does not name the version of the binary. With the archive format the same two steps run the built binary from the Cargo target folder before it is packed. The build job holds `contents: read` and no secret.
- `commit_file = true` adds a `COMMIT` asset holding the commit the tag is on. The `sums-<package>` job writes it before `SHA256SUMS`, so `SHA256SUMS` lists it, an attested release attests and verifies it, and `velnor-ci release publish` uploads it.

## Release checks

A repository that must pass its own checks before a release publishes declares one `[[checks]]` entry per check in `.velnor/release.toml`. Without an entry the generated `release.yml` is unchanged. Each entry becomes one job of `release.yml`, named `check-<name>` with the name lowercased and every run of other characters turned into `-`.

```toml
[[checks]]
name = "Qualify linux"
run = "bash scripts/qualify.sh"
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
- `permissions` adds token scopes to the job. The job holds `contents: read` and nothing else unless the table declares more. `contents: write` and `id-token: write` are refused, because the job runs repository code.

A check job checks out the verified commit without persisting credentials, and runs without the `release` environment, so it holds no release App secret. A check cannot use `uses:`, so a step that needs an action must be a script step or a separate workflow.

## Declared workflows

`.velnor/workflows.toml` makes the generator write one workflow file, `.github/workflows/<file>.yml`, for each `[[workflow]]` table. It is the only place that declares extra workflows: the `[[workflow]]` table of the earlier `fast/1290-source-preview` draft in `rust-ci.toml` is gone, and the reusable-workflow section that stood here is replaced by this one. The file is off when it is absent, and then the generated files are byte-identical to the output without it. `file` is lower case letters, digits and dashes; `rust-ci` and `release` belong to other outputs and are refused. A workflow sets `permissions: {}` at the top, and each job lists only the scopes it names. The samples `reusable-workflows`, `declared-workflow` and `declared-workflows` use every key below.

```toml
[[workflow]]
file = "release-checks"
name = "Release checks"
runner = "velnor-linux-x64"      # default ubuntu-latest
hosted = false                   # true opens the runner to fork pull requests; see "Fork pull requests"
fork_runner = "ubuntu-latest"    # default ubuntu-latest, must be GitHub-hosted
if = "github.event_name != 'merge_group'"

[workflow.concurrency]           # optional; a job may set the same inline: concurrency = { group = "...", cancel_in_progress = false }
group = "${{ github.workflow }}-${{ github.head_ref || github.run_id }}"
cancel_in_progress = true        # default false

[workflow.on]                    # absent: the workflow is reusable (workflow_call)
merge_group = true
schedule = ["23 4 * * 1"]

[workflow.on.push]
branches = ["main"]
tags = ["rust-ci-v*"]            # the tag prefix is a value you choose
paths_ignore = ["docs/**", "**.md"]

[workflow.on.pull_request]
types = ["opened", "synchronize", "reopened"]
branches = ["main"]
paths = ["crates/**", "Cargo.lock"]

[workflow.on.workflow_dispatch]

[[workflow.on.workflow_dispatch.input]]
name = "mode"
type = "choice"
options = ["quick", "full"]
default = "quick"

[[workflow.job]]
id = "plan"
if = "github.event_name == 'push'"
timeout_minutes = 5
permissions = { contents = "read" }
outputs = { matrix = "steps.plan.outputs.matrix" }

[[workflow.job.step]]
uses = "checkout"

[[workflow.job.step]]
id = "plan"
name = "Plan"
run = "printenv MODE"
env = { MODE = "${{ inputs.mode }}" }

[[workflow.job]]
id = "check"
needs = ["plan"]
environment = "release"

[workflow.job.matrix]            # one source: from_output, or axes and include
from_output = "plan.matrix"
fail_fast = false
max_parallel = 2

[[workflow.job.step]]
name = "Check"
run = "echo check"
continue_on_error = true
env = { TARGET = "${{ matrix.target }}" }

[[workflow.job]]
id = "assets"
needs = ["check"]
uses = "build-assets"            # a workflow declared earlier in this file
permissions = { contents = "read", id-token = "write", attestations = "write" }
with = { version = "${{ github.sha }}", dry_run = false }
```

### Triggers

Every workflow lists its events under `[workflow.on]`. The generator writes them in one fixed order whatever order you list them in: `workflow_call`, `workflow_dispatch`, `schedule`, `push`, `pull_request`, `merge_group`, `workflow_run`. Without `[workflow.on]` the workflow has `workflow_call` alone.

- `workflow_call = true`. The inputs are `[[workflow.input]]` and the outputs are `[[workflow.output]]` of the workflow. An input has `name`, an optional `description`, `type` (`string`, `boolean` or `number`, default `string`), an optional `default` of that type, and `required`, which is `true` unless there is a `default`. A number is a whole number of zero or more. An output has `name`, an optional `description` and `value = "<job>.<output>"`, where the job declares that output (or is a job that calls a workflow with that output); it renders `${{ jobs.<job>.outputs.<output> }}`.
- `[workflow.on.workflow_dispatch]` with `[[workflow.on.workflow_dispatch.input]]`. A dispatch has at most 25 inputs. Input names of one trigger are unique ignoring case, since GitHub reads them without regard to case. The input keys are the same, and `type` may also be `choice` (with `options`, and a `default` that is one of them) or `environment`.
- `schedule = ["<five-field cron>", ...]`. A schedule that can fire twice less than 5 minutes apart (`*/1`, `*/4`, `0,3`, `*/7`, which fires at :56 and :00) is refused, because 5 minutes is the shortest interval GitHub runs a schedule at. The gap is measured over the days the day, month and weekday fields select, so `0,57 0,23 * * 1` is allowed (Mondays only) and `0,57 0,23 * * *` is not (23:57, then 00:00). A schedule that no day matches, such as `0 0 31 2 *`, is refused.
- `[workflow.on.push]` needs `branches` or `tags` or both, which are GitHub filter patterns. An unfiltered `push` is refused. Nothing in the generator knows a tag prefix: `rust-ci-v*`, `v*` and `sdk-preview-*` are all just values.
- `[workflow.on.pull_request]` needs `types`, and takes optional `branches`. `pull_request_target` does not exist in this model, so a declared workflow never runs fork code with secrets.
- `paths` or `paths_ignore` (never both on one trigger, as in GitHub) on `push` and `pull_request` skip the run when no changed file matches, or when every changed file is ignored. They are written `paths` and `paths-ignore`, after `branches` and `tags`. A `paths` or `paths_ignore` of `"**"` is refused. GitHub ignores path filters for a tag push and for `merge_group`, and a required check that a path filter skips stays pending on the pull request, so a workflow behind a required check either has no path filter or is not required. `workflow_run` has no path filters.
- A `branches` list that holds `"**"` is refused as redundant on `push`, `pull_request` and `workflow_run`: leave `branches` out to match every branch (a `push` without `branches` needs `tags`). `tags = ["**"]` is allowed, since without `tags` a push of a tag starts nothing.
- A `branches`, `tags` or `paths` list of only negative patterns (each starting with `!`) is refused on every trigger: GitHub requires one pattern that does not start with `!`.
- `merge_group = true`.
- `[workflow.on.workflow_run]` needs `workflows` (the names of the workflows it follows), takes `types` (default `completed`) and optional `branches`. A `workflow_run` workflow runs in the context of the default branch. Do not check out another ref: `with` cannot read `github.event.*`, so a step cannot name the head of the run it follows. A chain of declared workflows that follow each other by name is refused when it is more than three `workflow_run` levels deep, which is as far as GitHub follows, or when it loops.
- Inputs of `workflow_call` and `workflow_dispatch` are both read as `inputs.<name>`.

### Concurrency

`[workflow.concurrency]` and the `concurrency` key of a job take `group` and `cancel_in_progress` (default `false`), and are written as `concurrency: {group, cancel-in-progress}` after `permissions`. A superseded run is cancelled only when two runs share a group, so a port that should drop runs for an older commit names a group per branch or pull request.

- A group is plain text of letters, digits and `- _ . : /` (up to 200 bytes) with these expressions in `${{ }}` and no others: `github.workflow`, `github.ref`, `github.event.pull_request.number` and `github.head_ref || github.run_id`. `github.head_ref || github.run_id` is the pull request branch, or a group of one run for a push; `github.ref` is one group per branch, tag or pull request ref.
- `cancel_in_progress = true` needs `github.ref` or `github.head_ref || github.run_id` in the group. A group of `github.workflow` alone would let a run on one branch cancel the runs on every other, release runs included, and is refused.
- A call job and a workflow that contains it are refused when their group is also the group of the declared workflow they call (or of a job of it, or of a workflow it calls in turn) and one of the two cancels in progress, because GitHub then cancels the run on a deadlock.
- A job that calls a reusable workflow may set `concurrency` too. Nothing but the group and the flag is configurable, so a group never reads `secrets`, `inputs`, `matrix` or `github.event.*` text an author of a pull request controls.

### Jobs

- `name` is the job's display name, which GitHub shows in place of the job id. It is optional.
- `needs` lists earlier jobs of the workflow. `if` is one expression, with or without `${{ }}`. A workflow `if` and a job `if` both apply: the job gets `(workflow if) && (job if)`.
- `runner` sets the runner of one job; the workflow `runner` is the default. A runner label is letters, digits, dashes, underscores and dots. A job's `hosted` declares its `runner` (see "Fork pull requests"); without `hosted`, a job whose `runner` is the workflow's label takes the workflow's value, a label spelled like a GitHub-hosted one is refused, and any other label is self-hosted. A runner that is not declared `hosted = true` gets the fork fence: `runs-on` is `<runner>` for a push, a schedule and a pull request from this repository, and `fork_runner` for a pull request from a fork.
- `runner = "${{ matrix.<axis> }}"` takes the runner from an axis (or an `include` key) of the job's static matrix, so one job builds for two runners. The expression is exactly that and nothing else, and the matrix must be `axes` and `include`, not `from_output`. `runner_labels = { "ubuntu-24.04" = true, "velnor-arm" = false }` declares every value of the axis, `true` for GitHub-hosted and `false` for self-hosted, as `hosted` does for one label; a value that is not declared, and a declared label that no value uses, is refused, and so is `hosted` beside such a runner. The job is open to a pull request from a fork only when every label is `true`, and then `runs-on` is `${{ matrix.<axis> }}`. With one `false` label, `runs-on` is `<same repository> && matrix.<axis> || '<fork_runner>'`, the fence of a fixed self-hosted runner, so every leg of a fork pull request runs on `fork_runner`. A container or service job is refused when any label is a `macos-*` or `windows-*` one. `runner_labels` without such a runner is refused. The sample `declared-workflows` has a `tools-build` workflow that builds two targets this way.
- `timeout_minutes` is 1 to 360, default 30.
- `permissions` accepts these scopes: `actions`, `artifact-metadata`, `attestations`, `checks`, `contents`, `deployments`, `id-token`, `issues`, `packages`, `pages`, `pull-requests`, `security-events` and `statuses`, each `read`, `write` or `none` (`id-token` has no `read`). A job with none gets `permissions: {}`. `checkout` needs `contents: read` on a private repository, so name it: a `checkout` step is refused unless its job has `contents` set to `read` or `write`. An `attest` step also needs `contents` `read` or `write`, besides `id-token` `write` and `attestations` `write`.
- `environment = "<name>"` is for step jobs. A deployment environment is what holds secrets and approval. A name is at most 255 characters.
- `outputs` maps a name to `steps.<step id>.outputs.<name>` of a step of the same job. A job with a `matrix` cannot declare `outputs`, because every matrix child would write the same name and the last to finish would win.
- `helper = true` puts the pinned `velnor-ci` install step first in the job.
- `[workflow.job.container]` takes `image`, optional `options` and `env`; each `[[workflow.job.service]]` takes `name`, `image`, `ports`, `options` and `env`. A step job only (a call job has none). Every `image` is `<name>@sha256:<64 lower-case hex digits>`, with no other `@` in the name, so a tag can never move the image. `options` is one line without expressions, written as `docker create` flags quoted with double quotes (no single quote or backslash), and only these flags pass: `--cpus`, `--health-cmd`, `--health-interval`, `--health-retries`, `--health-start-interval`, `--health-start-period`, `--health-timeout`, `--hostname`, `--memory`, `--shm-size`, `--stop-timeout`, `--ulimit`, `--user` (each with a value, `--flag value` or `--flag=value`) and `--init`, `--read-only`. A positional token (which Docker would read as the image), `--network`, `--entrypoint`, a mount, a published port and any other flag is refused. A service needs a `--health-cmd` with a command, or the job starts before the service is ready. A `ports` entry is `<port>` or `<host port>:<container port>`, each 1 to 65535; without a job container the runner picks a random host port for a bare `<port>` and no generated step can read it, so it needs the `<host port>:<container port>` form, and two services of a job cannot bind the same host port. A job with a container or a service needs a Linux runner: a `macos-*` or `windows-*` `runner` is refused, and so is a `fork_runner` of that kind when the job runner is not GitHub-hosted. A container or service `env` value is plain text and never reads a secret.
- `env = { NAME = "value", ... }` sets job-level environment variables of a step job (a job that calls a workflow cannot). A name is letters, digits and underscores. A value is plain text, or text that interpolates `${{ github.workspace }}` and no other expression, so a job `env` never carries a secret, an input or event text.
- `cache_mode` is one of `read`, `write`, `write-only` and `none`, and is written as the job-level `cache-mode` that GitHub reads (the workflow level is not written; the Actions cache default applies to a job that names none). `write` and `write-only` are refused unless the job `if` or the workflow `if` contains `github.ref != format('refs/heads/{0}', github.event.repository.default_branch)`, so a job that can write the cache never runs on the default branch. A job that saves to the cache (`cache` or `cache-save`) without `cache_mode = "write"` is refused, because the cache service would deny its save. A declared job with no `cache_mode` renders `read`, except a job whose only saver is an `mr-boxington` step on the `github` backend (or no backend, the action's default) renders `write`, when it runs on `push` or `schedule`. A job that also runs on `pull_request` counts as a saving job, so its pull request runs render `write` too, and whether MBX saves on a pull request is controlled by the MBX action's own `save-on-pull-request` input. This automatic `write` does not go through the default-branch `if` check that a declared `write` mode needs, because it applies only to a push or schedule saving job.
- A matrix has one source. `from_output = "<job>.<output>"` renders `fromJSON(needs.<job>.outputs.<output>)`; the job also gets `needs.<job>.outputs.<output> != ''` in its `if`, so an output that was never set skips the job instead of failing the run on `fromJSON('')`; that job must be in `needs` and declare the output. `axes = { os = ["a", "b"], ... }` renders a static matrix of up to 256 combinations, and `matrix.<axis>` is then a matrix key a value may read. `include = [{ target = "x", suffix = "y" }, ...]` lists paired plain values, as GitHub's `include` does: with `axes`, an entry may use axis keys (to extend the matching combinations or to add one) and new keys, and the new keys are readable too; without `axes`, each entry is one job, every entry sets the same keys, and an entry that repeats another is refused. An include key is an identifier other than `include` or `exclude`, spelled like an axis or earlier include key of the same name, and a value is non-empty plain text without `${{ }}`. The combinations of the axes plus the include entries stay within 256. Two keys whose names differ only by case are refused, since GitHub reads matrix keys without regard to case. A job `runner` may read an axis, see `runner` above. `fail_fast` and `max_parallel` are optional.
- `uses = "<workflow>"` makes the job call a reusable workflow, and then the job has no steps, `runner`, `timeout_minutes`, `environment`, `helper` or `outputs`. `<workflow>` is one of: the `file` of a workflow declared earlier in this file (its inputs are checked, its `boolean` and `number` inputs are written as such, and the job must hold every permission the called jobs ask for); `./.github/workflows/<name>.yml`; or `owner/repo/.github/workflows/<name>.yml@<full commit SHA>` (exactly 40 lower case hex digits). A `./.github/workflows` or pinned callee is not read, so its jobs get no fork fence and its permissions are not checked: a workflow that a `pull_request`, `merge_group` or `workflow_run` event starts is refused if it calls one, or calls a declared workflow that calls one. Such a call is allowed from `push`, `schedule`, `workflow_dispatch` and `workflow_call`. `with` passes inputs, each plain text or exactly one expression; an expression that certainly evaluates to text (a `needs` or `steps` output, a `github.*` scalar, `format`, `toJSON`, `join`, a quoted literal) is refused for a `boolean` or `number` input, and one that certainly evaluates to a boolean (a comparison, `!`, `contains`, `startsWith`, `endsWith`) is refused for a `number` input. No `secrets` are passed, ever. A later job reads `needs.<job>.outputs.<name>` for the outputs of a declared workflow. A chain of declared reusable workflows may be 10 levels deep at most (the top-level workflow and nine called ones), as in GitHub.

### Cases

A `[[workflow.case]]` is one qualification case that runs on a hosted runner and on a scale-set runner. The generator writes two jobs from it, `<id>-hosted` and `<id>-scale-set`, with the same steps, named `<title> / GitHub hosted` and `<title> / Velnor Scale Set`, and runs them in parallel.

- The workflow needs a `workflow_dispatch` input `mode` of type `choice`, and `hosted_runner` (declared `hosted = true` by its key) and `scale_set_runner` (declared `hosted = false` by its key). The scale-set job gets the fork fence of its runner like any other job.
- `modes` lists the `mode` options that run the case, and defaults to the case `id`. Each job gets `if: inputs.mode == '<mode>'` joined with `||`; a case `if` is joined to it with `&&`.
- `title` defaults to the `id`. `timeout_minutes`, `permissions`, `env`, `cache_mode` and the `[[workflow.case.step]]` tables follow the job rules above. A case has no `needs`, `environment` or `outputs`, and its two jobs always run the same steps; declare plain `[[workflow.job]]` entries for those. Cases are written before the plain jobs, so a plain job may need a case job (`<id>-hosted`, `<id>-scale-set`).

### Steps

- A step is exactly one of `run`, `uses` and `helper`. A name is optional for `uses` and `helper` steps. `continue_on_error = true` renders `continue-on-error: true` on that step only. `working_directory = "<path>"` renders `working-directory` on a `run` step: a plain relative path inside the workspace, with no leading `/` or `~`, no empty, `.` or `..` part, and no expression, `$`, backslash, backtick or colon. Use it instead of `env -C <dir>` or a `cd`; a `uses` or `helper` step takes none.
- `run` is one simple command: no line break, no `;`, `|`, `&`, `$(`, backtick, `if`, `for`, `while` or `case`, and no `${{ }}`. Pass values through `env`. A command that runs `velnor-ci` needs `helper = true` on its job, and its first argument must be a subcommand of `velnor-ci`; a test keeps the list in step with the helper. The command word is found after leading `NAME=VALUE` assignments and after every `env` prefix with its options (`-C <dir>`, `-u <name>`, `-i`, `-S <string>`, `--chdir=<dir>`, `--`), so `env -C stage velnor-ci check` is a helper call like `velnor-ci check`, and `env -C scripts ./publish.sh` is a repository script for the write token rule like `./publish.sh`. A `velnor-ci` reached through a path inside the workspace (`./velnor-ci`, `stage/velnor-ci`) is a binary of the repository, not the helper; an absolute path, `~`, `$` or a `..` part still names the helper. The same search looks through the other command wrappers, chains of them included: `nohup`, `exec`, `builtin`, `command` (`command -v` and `-V` only look a name up, so they run nothing), `nice [-n N]`, `ionice`, `timeout [options] DURATION`, `stdbuf`, `time` and `xargs` (the command after its options), so `nohup velnor-ci check`, `timeout 5 velnor-ci check` and `xargs ./publish.sh` are read like `velnor-ci check` and `./publish.sh`. `sh`, `bash`, `dash`, `zsh`, `ksh` and `ash` with `-c` run their script string, which is read the same way when it holds no `$` and no backtick; a script with an expansion, an unclosed quote or a shell keyword (`if`, `for`, `{`) cannot be read, and neither can `eval`, `source`, `.`, `sudo`, `doas`, `su`, `setsid`, `flock`, `find -exec`, `xargs` with a replaced command word, an option the scan does not know, or a command word that is an expansion. Such a command counts as a repository script for the write token rule, and a `velnor-ci` word anywhere in it counts as a helper call. Every command of a sequence (`;`, `&&`, `||`, `|`, `&`, a subshell, a line break) is read as well, although the render rules above already refuse the operators.
- `helper = "<command>"` renders `run: velnor-ci <command>` and needs `helper = true` on its job; `<command>` is a subcommand of `velnor-ci`, such as `validate-artifact-id`.
- A job whose permissions include a `write` scope refuses a step that reads `${{ github.token }}` in its `env` and runs a repository script (a command word, after the wrappers above, that is a path or `ash`, `bash`, `dash`, `fish`, `ksh`, `make`, `node`, `perl`, `python`, `python3`, `ruby`, `sh` or `zsh`, or a command that cannot be read) or a local action, so a script of the repository does not hold a write-capable token. Other commands, such as `gh release create`, are not refused.
- `uses` names a SHA-pinned action from this fixed list: `checkout`, `mise`, `upload-artifact`, `download-artifact`, `cache` (`actions/cache`), `cache-restore`, `cache-save`, `attest` (`actions/attest-build-provenance`), `github-script` and `mr-boxington` (`jdx/mr-boxington-action`). `with` sets its inputs. An input the pinned action does not have, or one with no default that is missing (`path` for `upload-artifact`, `path` and `key` for the cache actions, `script` for `github-script`), is refused; input names are compared without regard to case. A `github-script` `script` must be exactly one `console.log('<plain text>')` statement, with no quote, backslash, backtick or `$` inside the text, because decisions and loops live in the helper; its `github-token` must be `${{ github.token }}`, `${{ secrets.NAME }}` or `${{ env.NAME }}`. Adding an action is a change to this list, not to the model.
- `uses` also names a local action, and only these three paths are allowed: `./qualification/actions/post-fail`, `./qualification/actions/docker` and `./qualification/actions/composite`. They take no `with`. Any other local path, such as `./.github/actions/x`, fails generation, so a repository action cannot reach a job by accident. The job must run an unconditional `checkout` step without `path` before it, because a local action is read from the checked out repository. The generator writes the `# zizmor: ignore[self-repository]` note after each one.
- Locked inputs: `checkout` always has `persist-credentials: false`; `mise` always has `cache: false` and `cache_save: false`; `attest` always has `push-to-registry: false`. Setting one to anything but `false` is refused, and setting it to `false` is accepted and written once. `attest` is refused unless its job has `id-token = "write"` and `attestations = "write"`.
- An `if` on a step may read `needs.<needed job>.result`, `steps.<earlier step id>.outcome` and `github.*`, besides what a value may read. An `if` on a job may read `needs`, `inputs` and `github.*` only. In an `if`, `needs.*` is followed by `result` or `outputs.<name>` only, so a misspelled property cannot make a guard silently true.
- A `with` value may also be text that interpolates `${{ github.run_id }}`, `${{ github.run_attempt }}` and `${{ github.job }}` (a cache key such as `g4-${{ github.run_id }}-${{ github.job }}`), and no other expression: `secrets.*`, `github.event.*`, `inputs.*` and the rest are refused inside text. The cache key length rule counts `github.run_attempt` as 10 and `github.job` as 100 characters. An `env` value of a step cannot interpolate.
- An `env` or `with` value is plain text or exactly one expression. The allowed expressions are `github.token`, `github.repository`, `github.sha`, `inputs.<declared input>`, `matrix.<key>` in a matrix job, `steps.<earlier step id>.outputs.<name>` and `needs.<needed job>.outputs.<declared output>`. Anything else (including `github.event.*`), and any `needs` on a job declared later, is refused with the path of the problem. `secrets` is refused too, except in the one case below. This holds for a `workflow_run` workflow on a self-hosted runner as well, so the head branch, SHA and commit message of the run it follows (`github.event.workflow_run.*`), which an author of a pull request controls, reach a step only through an `if`.
- An expression may call `contains`, `startsWith`, `endsWith`, `format`, `join`, `toJSON` and `fromJSON`; an `if` may also call `always`, `success`, `failure` and `cancelled`; and a step (its `if`, `with` and `env`) may also call `hashFiles`, which a job `if` cannot. Any other function is refused. A cache action `key` or `restore-keys` is text, or text with `${{ github.run_id }}`, `${{ github.run_attempt }}` and `${{ github.job }}`; a whole-expression key such as `${{ inputs.cache_key }}` is refused, because its length is not bounded and the cache client rejects a key over 512 characters.
- A secret is read as `${{ secrets.<NAME> }}`, and only as the whole value of a step `env` entry, in a workflow whose events are all trusted: `workflow_dispatch`, `schedule`, and `push` that filters on `tags` only. A `push` with `branches` is trusted only when the job `if` or the workflow `if` is a `&&` conjunction with the default branch gate `github.ref == format('refs/heads/{0}', github.event.repository.default_branch)` as one whole term (no `||` anywhere in that condition). A `schedule` run executes the workflow file of the default branch, and a tag push is limited by the tag rulesets; neither is a run that an author of a pull request starts or picks the code of. `<NAME>` is an upper case letter followed by upper case letters, digits and underscores, and the value is exactly `secrets.<NAME>`: no `||`, no comparison, no `toJSON(secrets)`, no `secrets['NAME']`. Step `with`, `run` and `if`, a job `if`, a container or service `env`, a call `with` and a concurrency group never read a secret, and neither does any workflow that a `pull_request`, `merge_group`, `workflow_run` or `workflow_call` event, or an ungated `push` to branches, can start, because those events run for code that an author of a pull request or a caller of the workflow controls. A job `env` never reads a secret. Outside a workflow that runs on `workflow_dispatch` alone, a job that reads a secret holds no `write` permission. The error names the place and the event.

### Later

A `workflow_run` name is checked for form only, not against the workflows that exist: the generator reads the names of declared workflows from this file and does not read the `name:` of a hand-written file or of `rust-ci` (which is configurable), so a name that matches nothing is a workflow that never starts.

## CI on a pull request only on request

`pr_ci = "label"` in `.velnor/rust-ci.toml` turns the full suite on a pull request into something a maintainer asks for. It is `"always"` by default, and then the generated workflow is byte-identical to one without the key. The key needs a helper release that knows it (`0.1.88` or later), because older helpers reject unknown keys, and it cannot be combined with `skip_draft_pull_requests`, which also decides what a draft runs.

```toml
pr_ci = "label"
```

| Event | What runs | `Required` |
|---|---|---|
| Draft pull request | Nothing | Passes with the notice `draft: CI not requested` |
| Ready pull request | The quick set: the plan job, the lint jobs (in a one-job workflow, everything but the test steps) and the policy job | Passes when the quick set passes; the gate is not run |
| Ready pull request with the label `ci:run` | The full suite once for this commit; a job removes the label once `Required` has succeeded | The gate, as without `pr_ci` |
| Ready pull request with the label `ci:auto` | The full suite on every push; the label stays | The gate, as without `pr_ci` |
| `merge_group`, push to the default branch, schedule, `workflow_dispatch` | The full suite, exactly as without `pr_ci` | The gate, as without `pr_ci`; a skipped lane is never accepted |

A draft runs nothing even when it carries a label; the label takes effect when the pull request is marked ready, which starts a run. The pull request events are `opened`, `synchronize`, `reopened`, `ready_for_review`, `edited` and `labeled`. An `edited` event runs only when it retargets the base branch (`github.event.changes.base` is present), so a result for the old base is replaced; an edit of the title or body runs nothing, so it cannot cancel a requested full run. The workflow cannot filter by label name, so every added label starts a run, but a run for a label other than `ci:run` and `ci:auto` runs nothing: all its jobs, `Required` included, are skipped, and it gets a concurrency group of its own (`-ignored-<run id>` is appended), so it cannot cancel the run in progress. An `edited` event that does not change the base is treated the same way.

The job `ci-run-label` needs `Required` and removes `ci:run` only when it succeeded, so `ci:run` means one passed full run of that commit and a new push needs a new `ci:run`. A run that fails, or is cancelled by a draft conversion and a return to ready, leaves the label in place, so the replacement run is a full run too and a quick result cannot take the place of the full one on the same commit. Nothing else waits for it. The run in progress still runs the full suite, because every condition reads the labels from the event payload, which holds them as they were when the run was triggered. Removing a label starts no run: `unlabeled` is not a trigger type. It is the only job with `pull-requests: write`. It has that permission alone, no checkout and one `curl` step, so it runs no code of the pull request. It is skipped on a draft and on a pull request from a fork, whose token cannot write. A pull request from a fork never runs the full suite on a label, whoever applies it: it gets the quick set, and the full suite runs when it reaches the merge queue. Keeping a fork's code off self-hosted runners stays with the fork fence.

`pr_ci = "label"` is safe only together with a merge queue. An unlabeled pull request gets the quick set and a green `Required`, so the full suite is guaranteed only by the `merge_group` run. The repository needs a branch ruleset or branch protection with "Require merge queue" turned on for the default branch, and `Required` must be a required status check there. Without a merge queue, or with a queue that does not require `Required`, unlabeled code can merge without ever running the full suite. The generator cannot see repository settings, so it cannot refuse a repository without a queue; so the generated workflow checks at run time instead. With `velnor-ci` 0.1.100 or later, the plan job (the single job, right after its plan) runs `velnor-ci merge-queue-check` on every pull request run. It makes one `GET /repos/{repo}/rules/branches/<base>` call with `github.token` (no extra permission), and prints a `::warning::` when no active ruleset of the base branch has a `merge_queue` rule: label mode without a merge queue lets unverified code merge. A merge queue set up through classic branch protection is not visible to that API, so the warning can be ignored there. A lookup that fails (403, 404, a bad answer) prints a warning that says the check could not be made. The step has `continue-on-error`, never fails the run, and prints nothing when a queue is required. An older `velnor-ci` gets no such step. The generated workflow already runs the full suite on `merge_group`.

## Optional build-provenance attestation of release binaries

`attest = true` on a `[[binaries]]` entry in `.velnor/release.toml` makes the generated `release.yml` attest that binary's release assets. It is `false` by default, and then the workflow is byte-identical to one generated without the key.

```toml
[[binaries]]
package = "app"
attest = true
```

The `sums-<package>` job, which already downloads every platform asset and writes `SHA256SUMS`, gets one `actions/attest-build-provenance` step (pinned by commit) with `subject-path: dist/*`. It runs after the checksums are written and before they are uploaded, so the attestation covers every asset and `SHA256SUMS`. That job, and only that job, holds `id-token: write` and `attestations: write`; the workflow level keeps `permissions: {}`, and the build and publish jobs do not gain either scope. The build jobs run the compiler and stay without a token that can mint an identity. Verify a downloaded asset with `gh attestation verify <file> --repo <owner>/<repo>`.

With `attest = true` a separate `verify-<package>` job verifies every asset and `SHA256SUMS`, and `publish-<package>` needs it. One step per file runs `gh attestation verify` with a pinned `gh` (installed through mise), and checks `--repo`, `--source-digest`, `--source-ref`, `--signer-workflow` (this `release.yml`), `--signer-digest` and `--deny-self-hosted-runners`. A failed verification fails the job, `publish-<package>` is skipped and nothing is published. The `verify-<package>` job holds only `attestations: read`, with no `contents` scope. Its `GITHUB_TOKEN` is the one the pinned `gh` sees, so that binary can read attestations and nothing else. The `publish-<package>` job holds only `contents: write`, as it does without `attest`, and it never installs `gh` or runs mise. Both jobs download the artifacts separately, so a swap of an artifact between the two jobs is not detected. With `format = "raw"` the executable, its `.sha256` and its `.provenance.json` are each verified. With an `asset_name` that holds `{{ version }}`, each verified path is quoted with its `${VERSION}` reference in double quotes, after the step that reads the version from the tag.

`--source-digest` and `--source-ref` are the `GITHUB_SHA` and `GITHUB_REF` of a `workflow_run` job, which are the default-branch tip when the run started and not the tag commit. The archives are built from the tag, and the build jobs already check that the tag is the commit the gate verified. The attestation therefore proves that this workflow attested the files on a hosted runner at that default-branch commit. It does not prove that the files were built from the tag commit. `--deny-self-hosted-runners` rejects an attestation made on a self-hosted runner, and the `sums-<package>` job runs on a hosted runner, so no legitimate attestation is rejected.

A re-run of only the verify or publish job can fail. The artifacts expire after one day, and the attestation is checked against the commit of that run. The failure fails closed: nothing is published. Re-run the whole workflow run, or let the next default-branch run release.

With `family_tag`, set `attest` at the top level of `release.toml` instead (see Release family tag).

## Release settings

These top-level keys of `.velnor/release.toml` sit beside `[[binaries]]`, `[[checks]]` and the family keys.

- `roots` is the list of Cargo root folders that the release reads. It is `["."]` by default. Each root is a relative folder with no leading `/` or `-` and no empty, `.` or `..` part. `"."` alone is the repository root. An expression is refused.
- `after` is the list of workflow names that the release follows with `workflow_run`. It is `["Rust CI"]` by default. The list may not be empty or hold a blank name, and a name that no workflow has is an error.
- `pr_only_checks` is a list of required checks that run only on pull requests. The release gate does not wait for them on the tagged commit. Each name must be a required check of the default branch, and each name is listed once; blank and repeated names are refused. It is refused together with `family_required_checks = false`.

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

The generator refuses a name that is empty, does not start with a letter or `_`, holds anything beyond letters, digits, `-` and `_`, or is one Cargo reserves or that collides with a `target` subfolder: `dev`, `test`, `bench`, `debug`, `doc`, `build-override`, `package`, `deps`, `examples`, `build` and `incremental`. The generator reads the root `Cargo.toml` and the tracked `.cargo/config.toml` files, and refuses a profile other than `release` that none of them defines under `[profile.<name>]`. When the root `Cargo.toml` is not part of the generator's view, the check is skipped, and a missing profile then fails in the build step with Cargo's `profile ... is not defined` error before the release job publishes.

## Release family tag

`family_tag` in `.velnor/release.toml` is opt-in. Without it the generated `release.yml` is unchanged. It sets one tag for every `[[binaries]]` entry, for example `family_tag = "tools-v{{ version }}"`. The pattern holds `{{ version }}` once and no `{{ package }}`. The version is the version of the binary packages, read from `cargo metadata` at the verified commit, and every binary must have the same one.

```toml
family_tag = "tools-v{{ version }}"

[[binaries]]
package = "app"
```

Each binary package must be listed in its `release-plz.toml` with `release = false`, so that the family tag is its only tag. Other packages keep their release-plz tags. The pattern must not collide with a release-plz tag pattern. The generator accepts any valid pattern with one `{{ version }}` and does not enforce a naming scheme. The group prefix of #1429 (`tools-v`, `runner-v`, `actions-v`, never a bare `v`) is a convention that the repository tag rulesets enforce, because the config has no field that says which prefix a project requires.

A `family` job binds the tag to the commit that the gate verified. It uses `velnor-ci release family-tag`, which needs a helper release of `0.1.85` or later. It creates the tag on that commit, and it refuses the release unless the tag resolves to that commit itself. A published release of the tag on an earlier ancestor commit releases nothing. The build jobs check out the tag, refuse any other commit, and share one `sums-family` and one `publish-family` job, so all binaries of a release share one tag, one commit, one `SHA256SUMS` and one release.

`family_tag` does not combine with `checksum_files` yet: generation refuses the combination. `smoke` works on each binary as in the per-package release. Because one release holds every binary, `attest` and `commit_file` are set once at the top level, as is `checksums_file`, which replaces `member_digests`; generation refuses `attest`, `commit_file`, `member_digests` and `checksum_files` on a binary.

### Family keys

These top-level keys need `family_tag`, and generation refuses them without it.

```toml
family_tag = "tools-v{{ version }}"
checksums_file = "BINARY-SHA256SUMS"
commit_file = true
attest = true
family_trigger = "tag_push"
```

- `checksums_file` names one file that holds the SHA-256 of every binary inside every archive of the family, as `<archive>/<binary>` lines. It is published and listed in `SHA256SUMS`. The name is a plain file name of letters, digits, `.`, `_` and `-` that does not start with `.` or `-`, is not `SHA256SUMS` or `COMMIT`, and it needs every binary to use the archive format.
- `commit_file = true` publishes a `COMMIT` file that holds the commit of the release, listed in `SHA256SUMS`.
- `attest = true` adds one `actions/attest-build-provenance` step, pinned by commit, to `sums-family`, over every file of the release. Only that job holds `id-token: write` and `attestations: write`. A `verify-family` job holds `attestations: read` and runs a pinned `gh attestation verify` on every file and on `SHA256SUMS`, and `publish-family` needs it.
- `family_trigger` is `workflow_run` (the default) or `tag_push`. With `tag_push`, `release.yml` starts on `push` of tags that match the family tag, for example `tools-v*`, and has no release-plz job: the `gate` job checks that the tagged commit is on the default branch and that its required checks passed, and the `family` job runs `velnor-ci release family-tag --pushed-tag`, which refuses a tag that is not the one the package versions name. It needs a helper release of `0.1.98` or later, and it is refused while release-plz still releases a package of the roots. `after` does not apply to it. The file is still `release.yml`.
- With `family_trigger = "tag_push"`, the `family` job uses the `release` environment, and a tag push deploys from the tag ref, not from a branch. The environment must therefore allow the family tag pattern, for example `tools-v*`, in its deployment branch and tag policy. If it allows only `main`, or only protected branches, the job never starts and the run fails with a deployment-protection error. Set it in the GitHub UI under Settings > Environments > `release` > Deployment branches and tags > Selected branches and tags > Add deployment branch or tag rule, with the ref type "Tag" and the name pattern `tools-v*`. Or use the API: `gh api -X POST repos/<owner>/<repo>/environments/release/deployment-branch-policies -f name='tools-v*' -f type=tag` (the environment needs `deployment_branch_policy.custom_branch_policies` set to `true`). The generated `release.yml` carries a comment above `environment:` that says the same, and the nightly `scripts/tag-ruleset-drift.mjs` check fails when the policy list of the `release` environment lacks the tag pattern of the family in `.velnor/release.toml` (the family tag prefix plus `*`).
- `family_required_checks` is `true` (the default) or `false`, and needs `family_trigger = "tag_push"`. It controls only the required-checks wait of the `gate` job. With `true`, the gate runs `velnor-ci release-gate`, which requires every required check to have passed on the tagged commit. With `false`, the gate does not run it and answers ready once the default-branch check passes. The gate still checks that the tagged commit is on the default branch, and the `family` job still passes `--pushed-tag`, the attestation is still made, and `publish-family` still waits for `verify-family`. Use `false` only while releases are cut from commits whose required checks may still be pending, and restore `true` afterwards. It is refused together with `pr_only_checks`, which only the required-checks wait uses.

The sample `release-bin-family-tools` renders the tools release of this repository (two binaries, `BINARY-SHA256SUMS`, `COMMIT`, attestation, smoke, tag push).

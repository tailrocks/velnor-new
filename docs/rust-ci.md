# How Velnor keeps Rust CI fast

This page is for a project that runs `velnor-gen`. It says what the generated workflow does and which setting changes it. Every setting lives in `.velnor/rust-ci.toml`. The full reference is [`generator/CONFIG.md`](../generator/CONFIG.md), the exact cache keys are in [`rust-ci-cache-keys.md`](rust-ci-cache-keys.md), and the promises and targets are in [`goals.md`](goals.md).

`velnor-gen generate` writes `.github/workflows/rust-ci.yml` and `.velnor/rust-metadata.json`. `velnor-gen check` exits 1 when either file is out of date. Generation needs a pinned helper: `velnor-gen pin-helper <version>` writes `.velnor/rust-ci.lock`. Every decision at run time is made by one program, `velnor-ci`, so each generated `run:` step is one command.

## How the generator picks jobs

### What a run selects

The first step of a run is `velnor-ci plan`. It lists the changed files, finds the packages that own them, and adds every package that depends on one of them. Each later step runs only for the packages and roots the plan selected. A run that selects no package skips the Rust steps. When more than half of the packages are selected, the run selects everything. `velnor-ci plan --explain` prints each selected package with its reason, each step it left out and why, and what to add to the config for a file it cannot place.

- A pull request is judged by its diff. A push to the default branch and a merge queue run are judged by their diff when `on_main = "affected"` (the default), and select everything when `on_main = "everything"`.
- A scheduled run and a manual run select everything. `full_run` sets the schedule: `"nightly"` (default), `"hourly"`, `"weekly"`, `"off"` or a cron expression. For the built-in schedules `"hourly"`, `"nightly"` and `"weekly"`, the minute, hour and weekday come from a hash of the repository name, so projects do not start together. A cron expression runs as written.
- These changes select everything: a toolchain file, a Cargo config, a root `Cargo.toml` (unless only workspace dependency versions changed together with the lock), `mise.toml`, `mise.lock`, `.velnor/rust-ci.toml`, the generated workflow, and any file the plan cannot place. A changed `Cargo.lock` selects the packages that reach a changed entry, and everything when that cannot be told.
- `docs/`, `LICENSES/`, `NOTICE`, `CODEOWNERS`, `LICENSE*` and root-level `*.md` files select nothing (`default_ignores = true`), except a file inside a package directory or inside a Cargo root below the repository top, because the build can read it. `inputs` and `non_inputs` map other files to packages or to nothing.
- Documentation and doctest steps run only when the project has doc comments or doctests. On a full run they always run. They also always run when a procedural macro or build script generates code, or a dependency comes from outside the repository, because the generator cannot see every doc comment then.

### One job by default

The default shape is one job, `rust`, for each Cargo root. The generator never makes a job per crate. A crate gets a step (`Check <crate> alone`), not a job. The job count grows with Cargo roots, test groups and `[[lint]]` entries only. A second job costs start-up time, so the generator adds one only when it pays (`crates/velnor-rust/src/shape.rs`). The `shape` key chooses the layout; the table of the three shapes and the rule for `auto` is in [`generator/CONFIG.md`](../generator/CONFIG.md#run-selection).

`velnor-ci timings --repo owner/name` writes the timings file from the medians of at least three of the ten most recent first-attempt runs that pushed to the default branch or ran for a pull request into it. The generator writes the reason for the chosen shape into the workflow. A project with one root and one job has no `rust-plan` job; its plan is a step. Any other layout adds `rust-plan` first, and with several roots every job id gets a root suffix.

### Test groups

A group is a job for tests that need something the default job does not give: Docker, fewer threads, a longer limit or another machine.

```toml
[[test_groups]]
name = "database"
filter = "package(app) and test(/^db_/)"
needs = ["docker"]
```

The tests are built once and handed on as a nextest archive. `rust-test-default` runs everything that no group takes. With two or more groups in a root, `velnor-ci test-groups check` fails the build job when one test falls in two groups. `velnor-gen suggest-groups` reads the timings file and prints a group for every test binary that takes more than a third of its root's test time. Nothing creates groups on its own. The other keys of a group are `needs`, `test_threads`, `timeout_minutes`, `runner`, `hosted` (with a `runner`, needs `velnor-ci` 0.1.99), `root`, `paths` and `events`.

### Always present

`Required` (named by `required_check`) is the one check to require in a ruleset. It passes only when every selected job ran and passed. `Cache` reports cache waste. `Cache maintenance` (`rust-trim`) runs on the default branch only, and only when the pinned helper is velnor-ci 0.1.81 or later. A `policy` job runs `cargo-deny` when the project has a `deny.toml` and `cargo-machete` when `mise.toml` pins it, and `[[lint]]` entries add non-Rust lint jobs.

## Caches

Three families hold the work between runs. Each key is built from one typed value in `crates/velnor-model/src/cache.rs`, never from a hand-written string.

| Family | Key holds | Restored from | Saved by |
|---|---|---|---|
| Tools | runner OS and arch, mise version, hash of `mise.lock` | exact key only | default branch, on a miss |
| Sources (cargo registry and git) | OS and arch, hash of every `Cargo.lock` | exact key, else the newest earlier entry | default branch, when sources are proven public |
| Target (build directory) | job, root, OS and arch, `compat`, lock hash, commit, run id | newest entry with the same lock hash, else the newest with the same `compat` | default branch, when the run compiled something |

**What makes a hit.**

- `compat` is a hash of the `rustc` version, build target, the CI build profile, the repository's `[profile]` tables, `RUSTFLAGS`, the runner image and the workspace path. A different toolchain or profile starts a new line of entries.
- The lock hash leaves out the version of the workspace's own path packages (helper 0.1.88 or later). A release version bump does not move the key. A dependency bump does, and then the newest entry for the same `compat` is restored.
- A restore is useful only if cargo treats the files as fresh. `velnor-ci inputs save` records each tracked file's content and modification time with the build directory. `inputs restore` sets the time back on files whose bytes did not change. `velnor-ci restored` gives files that share a time distinct times, so cargo picks the same file every run.
- The CI profile sets `CARGO_INCREMENTAL=0`, `SOURCE_DATE_EPOCH=0` and debug info to `line-tables-only` (`ci_debuginfo`). Every build step uses the same `CARGO_TARGET_DIR`.
- On the default branch, a restored Target older than seven days is dropped. The run builds cold and selects everything.

**What can write.** Only `push`, `schedule` and `workflow_dispatch` runs on the default branch save a cache. A pull request, a merge queue run and a fork pull request only restore. A save needs at least one compiled, checked or documented crate, and incremental directories are removed first. When `Cargo.lock` holds a dependency that is not proven public, no Target or Sources entry is saved. The two newest entries of a group are kept. `Cache maintenance` prunes the repository towards `cache_budget_bytes` (default and maximum 8 GB of GitHub's 10 GB) on a best-effort basis. It deletes only entries it may delete (retired or superseded entries, idle pull request deltas, Target and Sources entries beyond the newest two of a group, and idle Tools entries), so it can stay over budget when the entries it must keep are larger than the budget.

**PR delta cache (helper 0.1.95 or later).** A pull request from this repository restores the Target entry, then a small per-pull-request delta layer (`velnor-dlt-` keys) on top of it. After the last compiling step the lane stages what the pull request changed in `target.velnor-delta` and saves it, so the next push to the same pull request starts from that work instead of the default branch's Target. A `Rust CI` cleanup workflow trims the deltas of a finished pull request. The lanes get these steps only when the lock pins `velnor-ci` 0.1.95 or later. An older helper (0.1.81 to 0.1.94) is not refused: the generator silently leaves the delta steps out and the output is what it was without the layer.

**MBX store.** MBX (Mr. Boxington) is a cargo wrapper that a project pins in `mise.toml`; it keeps a store of compiler results. Two settings feed that store:

- `mbx_shared_store = true`: when the runner host sets `VELNOR_MBX_SHARED_DIR`, a push, schedule or `workflow_dispatch` on the default branch uses that per-node directory. A pull request, a fork or a merge queue run never gets it. They can copy missing files from the read-only `VELNOR_MBX_SEED_DIR` into a private store. With helper 0.1.89 or later, `velnor-ci tools-local` also restores `mise` and `rustup` from the node's store, and the network cache is used only when that misses. A lane that restores from the node store reports it to the cache telemetry as a tools hit. A job that only runs archived tests and restored the tools from the network cache on a trusted event saves them to the node store too, so the next run on that node hits. With helper 0.1.100 or later, a failed save to the node store fails the Cache check, not the lane, and a run that could not read `/etc/os-release` saves no target or delta cache, because no later run could restore it.
- `[cache] mbx_remote = "github-actions"`: adds `jdx/mr-boxington-action`, which saves only after a push to the default branch. It is refused when the project has private dependencies.

**Debug a recompile.** `velnor-ci report` ends every cargo job and writes `ci-metrics.json`. The `Cache` check turns red when a crate was compiled although the restored cache held it, and when a save failed. It is advisory unless `cache_check_blocks = true`. To see why cargo rebuilt a crate, set `cache_debug = true`, regenerate, and set the repository variable `VELNOR_CACHE_DEBUG` to `1`. The jobs then set `CARGO_LOG` to the fingerprint log and upload it as `velnor-cargo-log-<job>` for three days.

## Pull requests on request

`pr_ci = "label"` makes the full suite on a pull request something a maintainer asks for. It cannot be combined with `skip_draft_pull_requests`.

| Event | What runs | `Required` |
|---|---|---|
| Draft pull request | nothing | passes with the notice `draft: CI not requested` |
| Ready pull request | the quick set: plan, lint jobs (in a one-job workflow, every step except tests) and the policy job | passes when the quick set passes |
| Label `ci:run` | the full suite once for this commit; the job `ci-run-label` removes the label | the gate |
| Label `ci:auto` | the full suite on every push; the label stays | the gate |
| Merge queue, push to the default branch, schedule, manual run | the full suite | the gate |

A pull request from a fork gets the quick set whatever its labels. The full suite runs when it reaches the merge queue. `ci-run-label` is the only job with `pull-requests: write`. It has no checkout and runs no code of the pull request.

## Private dependencies

```toml
[private_sources]
app_id = "vars.DEPS_APP_ID"
private_key = "secrets.DEPS_APP_PRIVATE_KEY"
repositories = ["owner/shared-lib"]
```

- A GitHub App token that can read only the listed repositories is created, and `velnor-ci fetch` runs `cargo fetch --locked` with it. The token is revoked when the fetch ends, before any code of the project or its dependencies builds or runs. Every later cargo command runs with `--offline`. The token is written to no file.
- The checkout stores no credentials and the jobs hold `contents: read`. No token is created for a fork pull request or for a pull request opened or pushed by a bot. In the merge queue a token is created only when the queued pull request comes from this repository (`velnor-ci private-token-allowed`).
- The `groups` shape builds and tests in one job and uploads no nextest archive, because workflow artifacts are readable by anyone who can read the repository.
- `velnor-gen generate` prints a note that suggests this table when `Cargo.lock` locks a github.com git dependency over ssh or from the repository's own owner. A registry or a non-github.com git source, which the token cannot authenticate, gets a note that says so instead. With the table present, a note names a locked repository of the token's owner that `repositories` does not list, and a listed repository that no lock uses. The token is created once per job that compiles; sharing one fetch between jobs through a cache or an artifact is not done, because both are readable by anyone who can read the repository.

## Speed budget

The numbers in [`goals.md`](goals.md) section "Targets" are the budget: a run that selects nothing finishes Velnor's own jobs within 1 minute, a warm run of unchanged code compiles and downloads 0 crates, the first cargo command starts within 30 seconds, cache storage stays under 8 GB, and the job count does not grow with crates. These are targets, and the generator does not measure all of them. What holds them today:

- The shape rule above adds a second job only when it shortens the wait.
- `Cache` fails on compiled-but-cached crates and failed saves. `Cache maintenance` enforces `cache_budget_bytes`.
- `velnor-ci bench collect` turns a run into a metrics file, and `velnor-ci bench compare` prints a table for two of them. Use them to compare runs before and after a change.

Set the `budget` table (see Settings) to turn four of these into a check. The `rust-budget` job compares the median and the latest of the last successful runs with the wall-time, runner-time and setup-time limits you set, and fails on a breach or on a measurement it does not have. It runs on the default branch or the scheduled run, never on a pull request, and never fails the required check.

## Settings

Every key of `.velnor/rust-ci.toml`, with its default. `generator/CONFIG.md` has the details.

| Key | Default | Meaning |
|---|---|---|
| `shape` | `"auto"` | `one-job`, `lint-test`, `groups` or `auto` |
| `test_groups` | none | `[[test_groups]]` entries, see above |
| `test_lane` | none | `[[test_lane]]` feature test lanes; a lane with `filter` also gets a `Test lane guard` step, and any lane needs velnor-ci 0.1.93 or later, see `generator/CONFIG.md` |
| `on_main` | `"affected"` | what a push or merge queue run selects: `affected` or `everything` |
| `full_run` | `"nightly"` | `off`, `hourly`, `nightly`, `weekly` or a cron expression |
| `pr_ci` | `"always"` | `always` or `label` |
| `skip_draft_pull_requests` | `false` | draft pull requests run nothing |
| `ci` | none | table; `skip_push_after_merge_group = true` skips checks the merge queue already ran |
| `docs` | `"always"` | `post_merge` runs `cargo doc` only on a `push`, `schedule` or `workflow_dispatch` run, not on a pull request or a merge queue run |
| `check_alone` | `"always"` | `post_merge` runs the per-crate `Check <crate> alone` steps only on a `push`, `schedule` or `workflow_dispatch` run, not on a pull request or a merge queue run |
| `sparse_checkout` | `false` | each Rust lane job checks out only the paths its lane reads |
| `inputs`, `non_inputs`, `exclude` | none | map files to packages, map files to nothing, or leave Cargo manifests out |
| `default_ignores` | `true` | documentation and licence files select nothing |
| `nextest_harnesses` | none | `package:target` entries for `harness = false` test targets that nextest can run; the others run in a direct step |
| `test_setup` | none | `[[test_setup]]` fixture steps that run before tests |
| `workspace_groups` | none | `[[workspace_groups]]` entries: Cargo workspaces that share one job set and one build folder, see `generator/CONFIG.md` |
| `lint` | none | `[[lint]]` jobs for checks that are not cargo |
| `cache` | none | table; `mbx_remote = "github-actions"` |
| `mbx_shared_store` | `false` | use the node's MBX store, see above |
| `cache_budget_bytes` | `8000000000` | cache budget, at most 8 GB |
| `budget` | none | table; `pr_wall_seconds`, `main_wall_seconds`, `runner_seconds`, `setup_seconds` bound the speed, checked by `rust-budget` (needs `velnor-ci` 0.1.92) |
| `cache_check_blocks` | `false` | the `Cache` check joins `Required` |
| `cache_debug` | `false` | cargo fingerprint log, see above |
| `ci_debuginfo` | `"line-tables-only"` | `full`, `line-tables-only` or `none` |
| `private_sources` | none | private dependencies, see above |
| `crates_mirror` | none | a crates mirror for a self-hosted runner (needs `velnor-ci` 0.1.93) |
| `runner` | `"ubuntu-latest"` | the runner label of every job |
| `hosted` | `true` when `runner` is not set | `true` opens `runner` to fork pull requests, `false` fences it; required when `runner` is spelled like a GitHub-hosted label; needs `velnor-ci` 0.1.99 |
| `fork_runner` | platform-dependent: `"ubuntu-latest"`, `"ubuntu-24.04-arm"` for a Linux arm64 `runner`, `"macos-15"` for an Apple silicon macOS `runner` | the hosted runner for fork pull requests when `runner` is self-hosted |
| `timeout_minutes` | `30` | job time limit, 1 to 360 |
| `required_check` | `"Required"` | the name of the one required check |
| `default_branch` | `"main"` | must equal the repository's default branch, or the run fails |
| `helper` | none | `{ path = "..." }` builds the helper from this repository instead of a release |

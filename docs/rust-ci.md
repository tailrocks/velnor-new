# How Velnor keeps Rust CI fast

This page is for projects that use `velnor-gen`. It describes what the generated workflow `.github/workflows/rust-ci.yml` does today. Every statement here holds for code on the default branch.

## What you do

1. Commit `.velnor/rust-ci.toml` (it may be empty), `mise.toml` and `mise.lock`.
2. Run `velnor-gen pin-helper <version>` to pin the `velnor-ci` helper in `.velnor/rust-ci.lock`.
3. Run `velnor-gen generate` and commit the result. `velnor-gen check` exits 1 when a generated file differs.

The generator reads your workspaces, crates, features, tests and tools, so a project with no configuration still gets a working workflow. The configuration keys are at the end of this page.

## What a run selects

The first step of a run is `velnor-ci plan`. It compares the files changed by the pull request, and chooses which Rust packages the run checks. A package is selected when:

- a file it reads changed (source, manifest, build script, or a file you map with `inputs`), or
- it depends on a selected package, or
- it is reached by a changed `Cargo.lock` entry.

A toolchain file, a cargo configuration or the workflow itself selects everything. A file the planner cannot place selects everything, until you map it with `inputs` or `non_inputs`. A change that touches more than half of the packages selects all of them. A change to docs or licence files outside every crate selects nothing, and then no Rust job runs.

The event decides how: pull requests and merge queue runs (`merge_group`) use the changed files. A push to the default branch does the same when `on_main = "affected"` (the default) and selects everything when it is `"everything"`. A scheduled run and a manual run (`workflow_dispatch`) always select everything. When git cannot tell what changed, the plan selects everything.

The plan step runs `velnor-ci plan --explain`. Its output lists each root, then each selected package with its reason (`changed:<path>`, `depends-on:<package>`, `input:<path>`, `lock:<dependency>`), or the trigger when everything is selected (`all:event`, `all:toolchain-file:<path>`, `all:unknown-path:<path>`, `all:most-packages:<n>`, and others). It also lists each step it left out, such as doctests or documentation, and why. To see the same output locally, run `velnor-ci plan --explain --changed <path>...` in a checkout. When a changed path is `Cargo.lock`, a root `Cargo.toml` or configuration, also pass `--base-file PATH=FILE` for each, with the pre-change contents; without it the local plan selects everything, unlike the workflow, which reads them from the first parent.

## Job shape

One Cargo root is one job, named `rust`, and it plans in its own first step. A repository with several roots gets a `rust-plan` job and one lane per root. The generator chooses between three shapes:

- `one-job`: format, lint, check, tests and doctests in one job.
- `lint-test`: a lint job and a test job.
- `groups`: one build job, then test groups.

With `shape = "auto"` (the default) the choice comes from `.velnor/rust-ci-timings.json`, which `velnor-ci timings` writes from the median of at least three ordinary default-branch runs. A second job is used when the shorter build takes more than 5 seconds. Without a timings file the shape is `one-job`. The reason is written as a comment above the job in the workflow.

Every generated `run:` step is one command. Decisions live in `velnor-ci`, a prebuilt helper pinned by commit. A job installs tools with `jdx/mise-action` from `mise.lock`, and runs cargo through `velnor-ci run`, which keeps `velnor-run.log` for the cache checks.

Three more jobs are always part of the workflow:

- `Required` runs `velnor-ci gate`. It passes only when every job the plan scheduled succeeded, no unscheduled job failed, and every package that listed tests ran at least one. Use this check, and only this one, as the required check in your ruleset. A skipped test step cannot pass it.
- `Cache` reads each lane's `ci-metrics.json` and fails when a crate was compiled although the restored cache held it, or when an upload failed. It is advisory unless `cache_check_blocks = true`.
- `rust-trim` (named "Cache maintenance") deletes old entries and enforces the cache budget. It runs only on the default branch, and has no checkout and runs no repository code.

When a repository has several roots, or its shape is not `one-job`, a `rust-inputs-audit` job also runs on full runs. It checks that every file a package reads at compile time outside its own folder is listed in `.velnor/rust-inputs.lock`. The audit sees only the reads cargo records for its own build (its features, target and profile). A build script that reads an external file without emitting `cargo:rerun-if-changed` is not seen, so map that file with `[[inputs]]` yourself.

## Merge queue

The workflow listens to `merge_group`. A queue run selects like a pull request (with `on_main = "affected"`), compares the queue's merge commit with its first parent, and only reads caches. Make the verdict check (`Required`, or the name set by `required_check`) the required status check of the ruleset that holds the queue. The workflow cancels an older run of the same pull request, and never cancels a queue or default-branch run.

## Caches

Four families of cache keys exist. [Rust CI cache keys](rust-ci-cache-keys.md) has the exact keys, fallbacks and paths.

| Family | Holds | Key covers | Written by |
|---|---|---|---|
| Tools | mise tools and the Rust toolchain | `mise.lock`, mise version, OS, architecture | A job that installs every tool, on the default branch, on a miss |
| Sources | cargo registry index, crate archives and git database | root, host, `Cargo.lock` hash | The test lane, on the default branch, on a miss |
| Target | the build directory | job, root, host, compat, `Cargo.lock` hash, commit and run | The lint and test lanes, on the default branch |
| Delta | key and budget rules only | job, root, host, compat, base | Nothing yet |

`compat` is a hash of the compiler version, build profile, `RUSTFLAGS`, workspace path, cargo `[profile]` tables and the runner image. A different compiler or profile restores an older build only by the last fallback, never by mistake.

Before cargo starts, `velnor-ci inputs restore` and `velnor-ci restored` give each tracked file the modification time it had when the cache was saved, so cargo sees an unchanged file as fresh. A restored base older than seven days is removed on the default branch, and the run then plans everything.

## What a pull request can write

| Event | Restores | Writes |
|---|---|---|
| Pull request, merge queue | Tools, Sources, Target | Nothing |
| Fork pull request | Caches (no token) | Nothing |
| Push, schedule or manual run on the default branch | Tools, Sources, Target | Tools, Sources, Target |

The saving needs helper `0.1.81` or later and no private or non-crates.io dependency source. Build lanes hold only `contents: read` and `actions: read`; only `rust-trim` can delete. The budget in `cache_budget_bytes` keeps the repository under 8 GB. It removes closed-pull-request entries first, then old Target and Sources entries beyond the newest two of each group, and never the newest base.

## Debug an unexpected recompile

Set the repository variable `VELNOR_CACHE_DEBUG` to `1`, with `cache_debug = true` in the configuration, and run the workflow again. Each cargo step then sets `CARGO_LOG` to cargo's fingerprint log, and the job uploads `velnor-run.log` as the artifact `velnor-cargo-log-<job>`, kept three days. The log names the file or setting that made a unit dirty. Read the `Cache` job summary first: it says how many crates were compiled and how many were needed.

## `velnor-gen` commands

- `generate`: write the generated files and remove those no longer generated.
- `check`: exit 1 when a generated file differs from what `generate` writes.
- `pin-helper <version>`: write `.velnor/rust-ci.lock` for a helper release.
- `release ...`: generate and check the `release.yml` workflow and its tag rulesets.
- `release ruleset`: print the two tag rulesets that let only the velnor-release App create release tags, one JSON object per line. Create each with `gh api` (the command's `--help` shows the loop).
- `release ruleset-drift <live.json>`: compare a JSON array of the live tag rulesets with the generated ones, bypass actors included. Exit 1 on a difference.
- `check-tag <tag>`: exit 1 unless the tag is `rust-ci-v` plus the program version.
- `record-sample`: record a golden test sample (for work on the generator).

## `velnor-ci` commands

- `plan`: select the packages the change affects (`--explain`, `--full`, `--changed`, `--base-file`).
- `prepare`: check tool versions, set the CI build profile and write `compat`.
- `check-metadata`, `check-configurations`: check `.velnor/rust-metadata.json` and the feature sets.
- `crates-mirror`: configure the optional crates mirror.
- `inputs build|save|restore|audit`: record, restore and audit the files cargo reads.
- `restored`: fix file times after the cache restores and expire an old base.
- `run -- <cmd>`: run a command, log its output, and check that doctests ran.
- `cache decide|trim|verify|check|budget`: decide a save, delete old entries, check uploads and the budget.
- `report`: write `ci-metrics.json` and the lane summary; `--check` looks for cache waste.
- `gate`: the `Required` verdict.
- `timings`: write `.velnor/rust-ci-timings.json`.
- `bench collect|compare`: collect and compare run metrics.
- `release ...`, `release-gate`, `check-tag`: release helpers.

## `.velnor/rust-ci.toml`

Unknown keys are refused. Every key is optional.

| Key | Default | Meaning |
|---|---|---|
| `runner` | `ubuntu-latest` | Runner label for every job |
| `fork_runner` | by platform | GitHub-hosted label used for pull requests from forks when `runner` is self-hosted, see [generator configuration](../generator/CONFIG.md) |
| `timeout_minutes` | `30` | Job time limit, 1 to 360 |
| `default_branch` | `main` | Checked against the repository's real default branch |
| `on_main` | `affected` | `affected` or `everything` for a push and a queue run |
| `full_run` | `nightly` | `off`, `hourly`, `nightly`, `weekly`, or a cron expression |
| `skip_draft_pull_requests` | `false` | Skip jobs on draft pull requests |
| `required_check` | `Required` | Name of the verdict job |
| `shape` | `auto` | `auto`, `one-job`, `lint-test` or `groups` |
| `exclude` | none | Globs of manifests (Cargo roots) the generator leaves out |
| `non_inputs` | none | Globs of files that select no package |
| `inputs` | none | Tables `{ glob, packages }`: files that select the named packages |
| `default_ignores` | `true` | Treat common non-source files as non-inputs |
| `nextest_harnesses` | none | `package:target` entries (`harness = false` targets) run by nextest; unlisted custom harnesses run directly |
| `test_setup` | none | Tables `{ root, name, run }`: one command before the tests |
| `ci_debuginfo` | `line-tables-only` | `full`, `line-tables-only` or `none` |
| `cache_budget_bytes` | `8000000000` | Repository cache budget, 0 to 8 GB |
| `cache_check_blocks` | `false` | Make the `Cache` job part of `Required` |
| `cache_debug` | `false` | Enable the fingerprint log described above |
| `crates_mirror` | none | Table `{ runner, registry, cainfo }`, see [generator configuration](../generator/CONFIG.md) |
| `helper` | none | Table `{ path }`: build the helper from source at this path |

## Coming next

Open work, by issue number only: #578, #601, #604, #607, #609, #610, #622, #623, #626, #644, #645, #646, #648, #649, #1161, #1210.

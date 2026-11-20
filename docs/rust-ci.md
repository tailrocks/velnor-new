# How Velnor keeps Rust CI fast

This page is for a project that runs `velnor-gen`. It says what the generated workflow does and which setting changes it. Every setting lives in `.velnor/rust-ci.toml`. The full reference is [`generator/CONFIG.md`](../generator/CONFIG.md), the exact cache keys are in [`rust-ci-cache-keys.md`](rust-ci-cache-keys.md), and the promises and targets are in [`goals.md`](goals.md). A project that moves from the velnor-actions engine starts with [`migrating-to-velnor-gen.md`](migrating-to-velnor-gen.md).

`velnor-gen generate` writes `.github/workflows/rust-ci.yml` and `.velnor/rust-metadata.json`. `velnor-gen check` exits 1 when either file is out of date. Generation needs a pinned helper: `velnor-gen pin-helper <version>` writes `.velnor/rust-ci.lock` in the `--root` directory (the one that holds `.velnor/`), and stages it only when that directory is itself a git repository root. Every decision at run time is made by one program, `velnor-ci`, so each generated `run:` step is one command.

## How the generator picks jobs

### What a run selects

The first step of a run is `velnor-ci plan`. It lists the changed files, finds the packages that own them, and adds every package that depends on one of them. Each later step runs only for the packages and roots the plan selected. A run that selects no package skips the Rust steps. When more than half of the packages are selected, the run selects everything. `velnor-ci plan --explain` prints each selected package with its reason, each step it left out and why, and what to add to the config for a file it cannot place.

- A pull request is judged by its diff. A push to the default branch and a merge queue run are judged by their diff when `on_main = "affected"` (the default), and select everything when `on_main = "everything"`.
- A scheduled run and a manual run select everything. `full_run` sets the schedule: `"nightly"` (default), `"hourly"`, `"weekly"`, `"off"` or a cron expression. For the built-in schedules `"hourly"`, `"nightly"` and `"weekly"`, the minute, hour and weekday come from a hash of the repository name, so projects do not start together. A cron expression runs as written.
- These changes select everything: a toolchain file, a Cargo config, a root `Cargo.toml` (unless only workspace dependency versions changed together with the lock), `mise.toml`, `mise.lock`, `.velnor/rust-ci.toml`, the generated Rust CI workflow `rust-ci.yml`, and any file the plan cannot place. Every other workflow that `velnor-gen` owns (its header carries a valid `velnor-gen-sha256` line, as `rust-cache-trim.yml`, `release.yml` and `qualification.yml` do; the header is read from the file's head version, or its base version when the file was deleted) selects nothing, since no Rust test reads it. A workflow that `velnor-gen` does not own selects everything as an unknown path. A changed `Cargo.lock` selects the packages that reach a changed entry, and everything when that cannot be told.
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
- The CI profile sets `CARGO_INCREMENTAL=0`, `SOURCE_DATE_EPOCH=0` and debug info to `line-tables-only` (`ci_debuginfo`). Every build step of a root uses the same `CARGO_TARGET_DIR`. A root at the repository root builds into `target`. Any other root builds into `target/velnor-roots/<root>` under the repository (locked helper 0.1.107 or later), outside every package directory, so Cargo's package fingerprint walk cannot meet a build that is writing there. A `build.target-dir` setting still wins. A lock that pins an older helper keeps `<dir>/target`, and moving the pin changes the target cache path of each nested root once, which costs one cold build.
- On the default branch, a restored Target older than seven days is dropped. The run builds cold and selects everything.

**What can write.** Only `push`, `schedule` and `workflow_dispatch` runs on the default branch save Tools, Sources and Target entries. A save waits for the build steps, not for the tests: once every build step has succeeded, the Target and Sources entries are saved even if a test fails, and a failed or skipped build saves neither. A build that the plan left out does not block the save, as long as one build finished. When the event asks for an audit (a schedule, a manual full run or an expired base), the audit must also pass. A pull request, a merge queue run and a fork pull request only restore them, except that a pull request from this repository saves its own delta (below). Each generated job carries a `cache-mode` key: `write` on a job that has a save step and `read` on every other job, and GitHub enforces that mode at the cache service. A job that calls a reusable workflow has no key, because GitHub documents none for it. A save needs at least one compiled, checked or documented crate, and incremental directories are removed first. When `Cargo.lock` holds a dependency that is not proven public, no Target or Sources entry is saved. The two newest entries of a group are kept. `Cache maintenance` prunes the repository towards `cache_budget_bytes` (default and maximum 8 GB of GitHub's 10 GB) on a best-effort basis. It deletes only entries it may delete (retired or superseded entries, idle pull request deltas, Target and Sources entries beyond the newest two of a group, and idle Tools entries), so it can stay over budget when the entries it must keep are larger than the budget.

**Verified tools (helper 0.1.109 or later).** A restored `~/.local/share/mise` or `~/.rustup` holds executables that every later step runs, so a restore is used only after it is checked against an anchor that a trusted run recorded:

- **The anchor.** `velnor-ci tools-verify record` computes one BLAKE3 digest over every entry of both directories (path, kind, mode, and the bytes of each file or the target of each link) right after the tools are installed and before any build or test runs. It refuses every event except a push, schedule or `workflow_dispatch` on the default branch, whatever the workflow condition says. When the anchor restore found no entry it writes one, and the next step saves it to the Actions cache as `velnor-tools-anchor-<tools key>`.
- **Why that anchor.** `mise.lock` hashes the downloaded archive, not the installed files. The tools entry holds no archive and no per-file hash, and mise downloads again on a reinstall even when its own download cache holds the archive, so checking installed files against the lock costs a cold install. rustup's channel manifest likewise hashes component tarballs, not installed files. Neither can check a tree without downloading it again. The anchor travels through the Actions cache of the default branch, which GitHub writes only from runs on that ref: a pull request, a fork, a merge queue run and the node-local store cannot write it. A digest stored inside the tools entry itself would prove nothing, because whoever writes the entry writes the digest too.
- **The check.** `velnor-ci tools-verify check` runs after the node-store restore and the cache restore and before `Install tools`. It keeps the directories only when their digest equals the anchor. A mismatch, a missing or malformed anchor, a special file or a read error deletes both directories and prints a `::notice::`, and the install runs from scratch. Whatever is present counts, so a partial restore or a leftover of an earlier job on a reused runner is checked too. `velnor-ci tools-verify reset` deletes `$RUNNER_TEMP/velnor-tools-anchor` right before the anchor restore, and `check` and `record` take the restore's `cache-hit` as `VELNOR_ANCHOR_HIT`: without a hit there is no anchor, whatever file is on disk, so a leftover `digest` can never vouch for a tree. A restored tree that cannot be read or deleted by ordinary means (a read-only or unsearchable directory) gets its directory modes raised first, so the discard still succeeds and the cold install runs. A file whose allocated blocks times 512 is less than its length counts as sparse and is refused. A filesystem with transparent compression (btrfs or zfs) can make an intact file look sparse, which costs a discard and a cold install, never a wrong verdict.
- **The saves.** A tools save (Actions cache or node store) needs `record`, which runs only on a trusted event on the default branch, to find the tools equal to the anchor, so a tree that a node store or a restore poisoned is never laundered into the other store. The node-store prime of a test-only job saves only a restore that `check` verified. A pull request never saves the tools anywhere. The policy job, which installs no Rust components, restores and verifies the tools but does not save them, so every saver writes the same tree.
- **What it does not cover.** A run on the default branch is trusted: code it runs before the save, such as a mise hook in the project's own config, could change what is saved and recorded. Builds and tests run after the saves. A job that steals a default-branch job's cache token could write both entries. The tools entry is one Actions cache archive, whose members can in principle name paths outside the two checked directories (such as `~/.cargo/bin`); only the two directories are verified, and the node store copies only those two. A poisoned Actions entry is never overwritten (entries are immutable), so every later run discards it and installs cold until the entry is deleted or the key moves. The anchor is fixed per tools key: when the runner image changes a file in `~/.rustup` (a hosted image ships one), every run discards the restore and installs from scratch until `mise.lock` or the mise version moves the key.
- **Self-hosted runners (helper 0.1.109 or later).** When `runner` is not declared `hosted`, a verified job restores the tools from the node store (`velnor-ci tools-local`, with or without `mbx_shared_store`) and skips the Actions cache entry for them: the restore, the lookup, the save and the prime carry `runner.environment != 'self-hosted'`, so a job of the same workflow that lands on a GitHub-hosted runner (a fork pull request) still uses the entry. The reason is measured: on the scale set, restoring the 590 to 620 MB archive took 31 to 66 s, while installing the same tools from scratch took 9 to 18 s. On a node miss the job installs the tools, and a trusted run saves them to the node store for the next job on that node. The anchor and the verify step are unchanged.
- An older pinned helper keeps the unverified restore, and `velnor-gen generate` and `check` print a note that names the gap.

**PR delta cache (helper 0.1.95 or later).** A pull request from this repository restores the Target entry, then a small per-pull-request delta layer (`velnor-dlt-` keys) on top of it. After the last compiling step the lane stages what the pull request changed in `target.velnor-delta` and saves it, so the next push to the same pull request starts from that work instead of the default branch's Target. A `Rust CI` cleanup workflow trims the deltas of a finished pull request. The lanes get these steps only when the lock pins `velnor-ci` 0.1.95 or later; the cleanup workflow needs 0.1.102 or later, because an earlier helper fails it once the pull request has merged. An older helper (0.1.81 to 0.1.94) is not refused: the generator silently leaves the delta steps out and the output is what it was without the layer.

**MBX by default.** Every generated job that compiles Rust runs cargo through mbx. The generated cargo steps do not name mbx; they run the cargo of the repository's own mise configuration, so the generator requires one of two layouts in `mise.toml` and refuses to generate without one. The native layout, which the refusal shows first: one unfiltered pin of `mr-boxington` under `[tools]` and `mr_boxington = true` on the pinned `rust` tool, with no `[wrappers.cargo]`. The explicit layout adds `[wrappers.cargo]` with `command = "mbx"` and `env = { MBX_CARGO_SHIM_MODE = "1" }` and is also accepted. A repository that takes its toolchain from a toolchain file with a path pins no rust and uses the explicit layout only. A `[wrappers.cargo]` with `command = "mbx"` and no shim env is refused, because it breaks `cargo -V`. The explicit wrapper is not reported as a weakening; any other wrapper is. Direct binaries such as `cargo-clippy` and `cargo-nextest` reach mbx because they call `cargo` back.

Each job that builds also gets one step, `Private mbx store`, that appends `MBX_CACHE_DIR=$RUNNER_TEMP/mbx` to `$GITHUB_ENV` before the tools install. Without it mbx would use its default store in the user's cache directory, which a self-hosted runner keeps between jobs and shares between a pull request and the default branch. The runner empties `$RUNNER_TEMP` for every job, so each job builds from an empty store and cannot read what an earlier job wrote. The price is that mbx does not reuse compiler results between jobs until `mbx_shared_store` or `[cache] mbx_remote` is set; a job with either setting gets no private store step and keeps the wiring described below. The default needs no helper newer than the one pinned.

`[cache] mbx = false` opts a repository out: no layout is required, no private store is set, and the setting is named as a check that does less (`plan --explain` and the review of a change that adds it). It is refused together with `mbx_shared_store = true` or `[cache] mbx_remote`, which need mbx. Writing the key, `mbx = false` or `mbx = true`, needs velnor-ci 0.1.109 or later in the lock, because an older helper rejects an unknown key; the default writes no key and needs no helper change. A release build gives each job a private mbx store only for the exact layout (`command = "mbx"` or the option on `rust`), and never with `mbx = false`. A tool whose key ends in `mr-boxington` next to the `mr-boxington` pin is refused, because two tools could provide `mbx`. Declared workflows are not covered; the `mbx_remote` action still installs the mbx version recorded in the generator, which must equal the `mr-boxington` pin.

**MBX store.** MBX (Mr. Boxington) is a cargo wrapper that a project pins in `mise.toml`; it keeps a store of compiler results. Two settings feed that store in place of the private one:

- `mbx_shared_store = true`: when the runner host sets `VELNOR_MBX_SHARED_DIR`, a push, schedule or `workflow_dispatch` on the default branch uses that per-node directory. A pull request, a fork or a merge queue run never gets it. They can copy missing files from the read-only `VELNOR_MBX_SEED_DIR` into a private store. With helper 0.1.89 or later, `velnor-ci tools-local` also restores `mise` and `rustup` from the node's store, and the network cache is used only when that misses. A lane that restores from the node store reports it to the cache telemetry as a tools hit. Each `velnor-ci tools-local` log line ends with `node=<name>`: the pod's `NODE_NAME` when it is set, else the hostname, else `unknown` (velnor-ci 0.1.106 or later). A job that only runs archived tests and restored the tools from the network cache on a trusted event saves them to the node store too, so the next run on that node hits. With helper 0.1.100 or later, a failed save to the node store fails the Cache check, not the lane, and a run that could not read `/etc/os-release` saves no target or delta cache, because no later run could restore it.
- `[cache] mbx_remote = "github-actions"`: adds `jdx/mr-boxington-action` (v1.7.3, pinned by SHA). It saves after a successful push to the default branch (the action's built-in rule, with no input) and after a successful pull request from the same repository. The pull-request save is scoped to that pull request's merge ref, so main and other pull requests cannot restore it. A pull request restores its own latest entry, else the default branch's latest entry. It does not save on a fork pull request, a schedule, a merge queue run, a manual run or a push to another protected branch (those inputs are `false`), so those only restore. It is refused when the project has private dependencies.

**Debug a recompile.** `velnor-ci report` ends every cargo job and writes `ci-metrics.json`. The `Cache` check turns red when a crate was compiled although the restored cache held it, when a save failed, and on the other avoidable costs listed in [rust-ci-cache-keys.md](rust-ci-cache-keys.md). A unit whose build failed is not judged, since cargo's own error explains the missing evidence. It is advisory unless `cache_check_blocks = true`. To see why cargo rebuilt a crate, set `cache_debug = true`, regenerate, and set the repository variable `VELNOR_CACHE_DEBUG` to `1`. The jobs then set `CARGO_LOG` to the fingerprint log and upload it as `velnor-cargo-log-<job>` for three days.

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

- A GitHub App token that can read only the listed repositories is created, and `velnor-ci fetch` runs `cargo fetch --locked` with it. The token is revoked when the fetch ends, before any code of the project or its dependencies builds or runs. The fetch runs the real `cargo` and `git`, never a mise shim or a command wrapper from the repository or `PATH`; when only such a shim is found, the fetch fails closed and still revokes the token. Every later cargo command runs with `--offline`. The token is written to no file.
- With this table, a job that runs `cargo-deny` first runs `cargo-deny fetch db` with no token and no private-source environment, so the offline `cargo-deny check` can read the RustSec advisory database.
- The checkout stores no credentials and the jobs hold `contents: read`. No token is created for a fork pull request or for a pull request opened or pushed by a bot. In the merge queue a token is created only when the queued pull request comes from this repository (`velnor-ci private-token-allowed`).
- The `groups` shape builds and tests in one job and uploads no nextest archive, because workflow artifacts are readable by anyone who can read the repository. `[[test_groups]]` is refused together with `[private_sources]` for the same reason, because a group runs from that archive.
- `velnor-gen generate` prints a note that suggests this table when `Cargo.lock` locks a github.com git dependency over ssh or from the repository's own owner. A registry or a non-github.com git source, which the token cannot authenticate, gets a note that says so instead. With the table present, a note names a locked repository of the token's owner that `repositories` does not list, and a listed repository that no lock uses. The token is created once per job that compiles; sharing one fetch between jobs through a cache or an artifact is not done, because both are readable by anyone who can read the repository.

## Speed budget

The numbers in [`goals.md`](goals.md) section "Targets" are the budget: a run that selects nothing finishes Velnor's own jobs within 1 minute, a warm run of unchanged code compiles and downloads 0 crates, the first cargo command starts within 30 seconds, cache storage stays under 8 GB, and the job count does not grow with crates. These are targets, and the generator does not measure all of them. What holds them today:

- The shape rule above adds a second job only when it shortens the wait.
- `Cache` fails on compiled-but-cached crates and failed saves. `Cache maintenance` enforces `cache_budget_bytes`.
- `velnor-ci bench collect` turns a run into a metrics file, and `velnor-ci bench compare` prints a table for two of them. Use them to compare runs before and after a change.

Set the `budget` table (see Settings) to turn four of these into a check. The `rust-budget` job compares the run that executes it, and the median of that run and the last successful runs before it, with the wall-time, runner-time and setup-time limits you set, and fails on a breach or on a measurement it does not have. It runs on every push to the default branch and on the scheduled run (when `full_run` schedules one), never on a pull request, also when a job before it was skipped and only when the required check succeeded, and never fails the required check. A push that `skip_push_after_merge_group` skipped because the merge queue had already passed its commit is not a sample. A run that failed is not a sample, so a breach ends with the next run that is fast again. A push that selects nothing and a push that selects a core crate differ by a factor of ten, so the median of both is meaningless: `[budget.full]`, `[budget.affected]` and `[budget.none]` set limits per selection, and a run is compared only with earlier runs that made the same selection (see `generator/CONFIG.md`). The job's title records that selection (`Speed budget (full)`, `(affected)`, `(none)` or `(unknown)`), which is how a later run is matched.

## Everything goes through mbx

Every Rust build and test runs through mise and mbx. `mise.toml` sets `[wrappers.cargo] command = "mbx"`, so `cargo` on `PATH` is the mise wrapper that runs mbx, and mbx keeps the compiler results in its store. Do not run `~/.cargo/bin/cargo` or the rustup toolchain directly.

Four exceptions are allowed. Nothing else may skip mbx.

1. **Token fetch.** The `velnor-ci fetch` step runs the real `cargo fetch --locked` for each Cargo root, with the private token in the environment of that one command. It never runs a mise shim or a command wrapper, so the token never reaches a wrapper from the repository or `PATH`. The fetch downloads and does not compile, so mbx has nothing to cache.
2. **Bootstrap of mbx and mise.** Building mise and mbx from source (`scripts/build-owned-tool.py`) uses the pinned official mbx release, checked by digest, as its cargo driver. A tool cannot be its own compiler wrapper while it is being built.
3. **Rustdoc and doctests.** `cargo doc` and doctests run without mbx, because mbx cannot cache them yet. This is accepted for now and is revisited when mbx caches them.
4. **Release-plz `cargo package` and `cargo publish`.** The `release_pr` and `tag` jobs run release-plz, which runs `cargo package` and `cargo publish` itself, without mbx and without `jdx/mise-action`. Both jobs hold the release App token, and the generator rule `MiseActionInTokenJob` forbids mise in a job that holds a token, so a repository-declared cargo wrapper must not run there. Token safety wins over mbx for these two jobs. Build jobs hold no token and do run through mise and mbx.

**Release builds always use a fresh private mbx store; never shared, never restored.** mbx's action records are unsigned: rewriting the records in a writable store makes a clean checkout serve the writer's output. A self-hosted runner's `~/.cache/mbx`, a shared store or a restored Actions cache would therefore be a poisoning path into release binaries. Every generated release job that runs cargo through mise (the `build-*` jobs and `family-versions`) has the step `Private mbx store` right after its checkout, before `jdx/mise-action` and any cargo: `echo "MBX_CACHE_DIR=$RUNNER_TEMP/mbx-release-<job>" >> "$GITHUB_ENV"`. The runner empties `$RUNNER_TEMP` for every job, so the store starts empty and no other job can write it. Release jobs never set `MBX_REMOTE` or mount the shared store, and never restore or save an mbx cache. The generator tests in `crates/velnor-gen/tests/it/release_mbx.rs` pin all of this.

No script may add a fifth exception. `scripts/build-owned-archive-guard.sh` builds its guard through the repository's mise and mbx, with a fresh private mbx store per run. `scripts/local-cargo.sh` was removed: it called rustup's cargo directly and only its own test used it. Exceptions 1 and 3 do not build mbx or mise, so the bootstrap rule does not cover them; the owner must decide whether they stay.

To verify a local checkout:

```sh
export MISE_TRUSTED_CONFIG_PATHS=$PWD
mise install
type -a cargo | head -1   # must show a path under mise/command-wrappers/bin/
```

Without `MISE_TRUSTED_CONFIG_PATHS`, mise falls back to plain cargo without a warning, so the `type -a` check is the test. A build through mbx prints `mbx[cache]: object cache: H hits, M misses, ...` lines in its output. If they are missing, the build did not go through mbx.

## Optional release assets

A binary in `.velnor/release.toml` can list `optional_platforms`, for example `macos-arm64` built on `macos-15`. Its build is best effort: the job has `continue-on-error`, `sums-family` runs with `always()` while it still needs every required build to have succeeded, and `velnor-ci release publish --optional-asset` (helper 0.1.108 or later) publishes, checksums and attests a file that exists and skips, with a notice, one that does not. A missing required file still fails the release. Under an older pin the generator refuses the key. See "Optional platforms" in `generator/CONFIG.md`.

## Settings

Every key of `.velnor/rust-ci.toml`, with its default. `generator/CONFIG.md` has the details.

| Key | Default | Meaning |
|---|---|---|
| `shape` | `"auto"` | `one-job`, `lint-test`, `groups` or `auto` |
| `test_groups` | none | `[[test_groups]]` entries, see above |
| `test_lane` | none | `[[test_lane]]` feature test lanes; a lane with `filter` also gets a `Test lane guard` step, `job = "parallel"` runs the lane as its own job (needs velnor-ci 0.1.107 or later), and any lane needs velnor-ci 0.1.93 or later, see `generator/CONFIG.md` |
| `on_main` | `"affected"` | what a push or merge queue run selects: `affected` or `everything` |
| `full_run` | `"nightly"` | `off`, `hourly`, `nightly`, `weekly` or a cron expression |
| `pr_ci` | `"always"` | `always` or `label` |
| `skip_draft_pull_requests` | `false` | draft pull requests run nothing |
| `ci` | none | table; `skip_push_after_merge_group = true` skips checks and archived test jobs the merge queue already ran (needs `velnor-ci` 0.1.103) |
| `docs` | `"always"` | `post_merge` runs `cargo doc` only on a `push`, `schedule` or `workflow_dispatch` run, not on a pull request or a merge queue run |
| `check_alone` | `"always"` | `post_merge` runs the per-crate `Check <crate> alone` steps only on a `push`, `schedule` or `workflow_dispatch` run, not on a pull request or a merge queue run |
| `sparse_checkout` | `false` | each Rust lane job checks out only the paths its lane reads |
| `inputs`, `non_inputs`, `exclude` | none | map files to packages, map files to nothing, or leave Cargo manifests out |
| `default_ignores` | `true` | documentation and licence files select nothing |
| `nextest_harnesses` | none | `package:target` entries for `harness = false` test targets that nextest can run; the others run in a direct step |
| `test_setup` | none | `[[test_setup]]` fixture steps that run before tests; `phase = "build"` runs one before the first compiling cargo step of each job, so clippy and `build.rs` see it |
| `workspace_groups` | none | `[[workspace_groups]]` entries: Cargo workspaces that share one job set and one build folder, see `generator/CONFIG.md` |
| `external_workflows` | none | paths under `.github/workflows/` of hand-written workflows that `velnor-gen check` must not flag; needs velnor-ci 0.1.109 or later, see `generator/CONFIG.md` |
| `lint` | none | `[[lint]]` jobs for checks that are not cargo |
| `cache` | none | table; `mbx_remote = "github-actions"`, `mbx = false` (see MBX by default) |
| `mbx_shared_store` | `false` | use the node's MBX store, see above |
| `cache_budget_bytes` | `8000000000` | cache budget, at most 8 GB |
| `budget` | none | table; `pr_wall_seconds`, `main_wall_seconds`, `runner_seconds`, `setup_seconds` bound the speed, checked by `rust-budget` (needs `velnor-ci` 0.1.92). `[budget.full]`, `[budget.affected]` and `[budget.none]` take the same keys for runs whose plan selected everything, some packages or none, and each kind of run is measured against runs of its own selection (needs `velnor-ci` 0.1.104) |
| `cache_check_blocks` | `false` | the `Cache` check joins `Required` |
| `cache_debug` | `false` | cargo fingerprint log, see above |
| `ci_debuginfo` | `"line-tables-only"` | `full`, `line-tables-only` or `none` |
| `private_sources` | none | private dependencies, see above |
| `release_assets` | none | `[[release_assets]]` entries: private release assets that a build script or test reads, fetched with the `[private_sources]` App, checked against a required `sha256` and cached by it, see `generator/CONFIG.md` (needs `velnor-ci` 0.1.109) |
| `crates_mirror` | none | a crates mirror for a self-hosted runner (needs `velnor-ci` 0.1.93); it stays in effect for a step that passes its own `--config` |
| `runner` | `"ubuntu-latest"` | the runner label of every job |
| `hosted` | `true` when `runner` is not set | `true` opens `runner` to fork pull requests, `false` fences it; required when `runner` is spelled like a GitHub-hosted label; needs `velnor-ci` 0.1.99 |
| `fork_runner` | platform-dependent: `"ubuntu-latest"`, `"ubuntu-24.04-arm"` for a Linux arm64 `runner`, `"macos-15"` for an Apple silicon macOS `runner` | the hosted runner for fork pull requests when `runner` is self-hosted |
| `timeout_minutes` | `30` | job time limit, 1 to 360 |
| `root_timeouts` | none | `{ "folder" = minutes }` (1 to 360) sets the job time limit of one root; `"."` is the repository root; needs `velnor-ci` 0.1.109, see [generator/CONFIG.md](../generator/CONFIG.md) |
| `required_check` | `"Required"` | the name of the one required check |
| `default_branch` | `"main"` | must equal the repository's default branch, or the run fails |
| `helper` | none | `{ path = "..." }` builds the helper from this repository instead of a release |

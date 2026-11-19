# Rust CI cache keys

`velnor-model` holds one type, `CacheKey`, for every cache entry of the generated Rust workflow. `velnor-render` has the only function that turns it into text (`render_key`). Every part that changes from commit to commit is an expression that GitHub fills in at run time, so a dependency bump changes no byte of the workflow file.

`host` is `${{ runner.os }}-${{ runner.arch }}`. `root` is the root's directory lower-cased, with every character other than an ASCII letter or digit turned into `_` (`root` for the repository root). `job` is the job id. `compat` is `steps.prepare.outputs.compat`.

| Family | Key | Restore fallbacks, in order | Path |
|---|---|---|---|
| Tools | `velnor-tools-<host>-mise<version>-${{ hashFiles('mise.lock') }}` | none | `~/.local/share/mise`, `~/.rustup` |
| Sources | `velnor-src-<host>-all.roots-${{ steps.sources_locks.outputs.hash }}` | `velnor-src-<host>-all.roots-` | `~/.cargo/registry/index`, `~/.cargo/registry/cache`, `~/.cargo/git/db` |
| Target | `velnor-tgt-<job>.<root>.<host>-<compat>-<lockhash>-${{ github.sha }}-${{ github.run_id }}` | `...-<compat>-<lockhash>-`, then `...-<compat>-` | `build.target-dir`, default `<dir>/target` |
| Delta | `velnor-dlt-<job>-<root>-<host>-<compat>-<base8>-${{ github.run_id }}` | `...-<compat>-<base8>-` | same as Target |

`steps.sources_locks.outputs.hash` is the `hashFiles` of every Cargo root's `Cargo.lock` in path order, computed once per job by the `Hash the sources lockfiles` step. `<lockhash>` is `${{ hashFiles('<dir>/Cargo.lock') }}` and `<base8>` is `${{ steps.restored.outputs.base8 }}`. A fallback is always a strict prefix of its key, and the second fallback of a Target key is a strict prefix of the first. A key never contains a branch name, a literal digest or `run_attempt`. `hashFiles` reads a root path literally: `*`, `?` and `[` are written as `[*]`, `[?]` and `[[]`, and a leading `!` or `#` gets a `./` prefix. Generation fails when a key can pass 512 characters at run time (the `actions/cache` limit), counting every expression at its longest value. `build.target-dir` is read from the `.cargo/config.toml` or `.cargo/config` that applies to the root (the nearest wins, the file without `.toml` first); generation fails when it is absolute, outside the repository, holds the root, starts with `~`, or contains a `{` or `}` placeholder, and when `build.build-dir` is set, because the Target cache would not cover that directory. Every generated step that builds a root (clippy, doc, check, build, nextest, doctests and the audit build, not format, deny or machete) sets `CARGO_TARGET_DIR` to `${{ github.workspace }}/<target directory of that root>`, the same path the Target cache of that root restores and saves, so a wrapper such as MBX leaves the directory in place and the cache can save it. The value is on the step, not the job or the workflow, because one job can build several roots with different target directories. A step value wins over `$GITHUB_ENV`, and `[env]` entries starting with `CARGO` are refused, so a repository cannot set another target directory for a generated step. The run id at the end of a Target key lets a full run save a clean base when the commit has not changed since the last save. A rerun of the same run reuses that id on purpose: the save finds the key taken and writes nothing, so unchanged data is never saved twice, and a rerun after a failed save still saves because no entry exists yet.

## Tools cache

Every job that installs the tools restores the Tools entry in the step before `jdx/mise-action` (a shard job, which installs only cargo-nextest, restores it too; the plan job installs nothing and restores nothing). `~/.local/share/mise` holds the mise program itself (`bin/mise`, which the action finds and only verifies against the signed checksums, so it downloads nothing), the installed tools and the link to the Rust toolchain; `~/.rustup` holds the toolchain, because mise installs Rust through rustup. Rust components and targets are added in every job before the save, so every job saves the same content; with `profile = "complete"` the save follows `velnor-ci prepare`, which installs the components the profile implies.

Only a job that installs every tool saves it, only on the default branch and only when the restore was not an exact hit. A tool-set change on a pull request installs fresh for that pull request and saves nothing. The key covers `mise.lock`, the mise program version the generator passes to `jdx/mise-action` as `version:`, the runner OS and the runner architecture: a changed tool version or a mise bump moves it (the entry is saved only on a miss, so without the mise version a bump would keep restoring a directory built by the old mise), a comment in `mise.toml` does not. It does not cover the runner image version or the targets in `rust-toolchain.toml`; a change in either only causes a re-download, never a wrong tool. Generation fails when `mise.lock` is missing, or when a pinned tool has no prebuilt binary for the runner's platform in it (a `cargo:` tool, for example); the error names the tool.

## Saving the build directory

With helper 0.1.81 or later, the ordinary lint and test lanes save Target entries on the default branch for `push`, `schedule` and `workflow_dispatch`. Saves require successful setup and producer steps. Pull requests, merge groups and forks only restore. A saving Nextest run prebuilds with `--workspace --locked --ignore-default-filter --no-run`; the normal test command still builds and runs directly on events that cannot save.

A single-job lane finishes its test commands and its full-run compile-input audit before publishing. A split lane runs the authoritative nextest run before it builds the doctests and the direct harnesses that nextest does not build, and publishes only after both, so the published base holds them and the next run compiles neither again; untrusted harness or doctest code never executes before the nextest run that the Required verdict reads. A failing test, doctest or direct-harness test blocks that publish, as it blocks the single-job lane's. Split and multi-root lanes depend on the independent compile-input audit job, which runs for schedules, manual full runs and pushes with `on_main = "everything"`. Failed or cancelled audits block every writer. A skipped audit permits an ordinary affected run to reuse its existing base, but an expired base cannot be published by a split lane without a successful full audit. The Required gate checks the same audit event conditions.

The save sequence is:

1. `velnor-ci inputs save --target-dir <configured target directory>` writes `.velnor/inputs.tsv` inside the directory being cached.
2. `velnor-ci cache decide` reads `velnor-run.log` and rechecks dependency source safety. `save=true` requires at least one compiled, checked or documented crate. `sources=true` independently authorizes a missed Sources entry when all sources are proven public. A warm Target can therefore replenish Sources. Unreadable evidence produces no authorization.
3. Before a Target save, the helper removes Cargo incremental directories at the profile and target-triple/profile levels and removes the Nextest archive. Build-script output directories with the same name remain. Traversal, metadata and cleanup errors fail the decision. The helper records the cold-build date in `.velnor/born`, carries it forward on later saves and reports the directory size. `velnor-ci run` overrides Cargo's quiet configuration and refuses explicit quiet arguments so compilation remains visible.
4. `actions/cache/save` compresses the directory directly. The lane records each saved key as a job output without a token. The `rust-trim` Cache job, which runs no repository code, then runs `velnor-ci cache verify --key` with its own token against the read-only cache API to require an exact entry on the saving ref. This catches upload errors that the pinned action reports only as warnings. A failed verification fails the Cache check without changing Required.

Build lanes hold only `contents: read` and `actions: read`. The separate `rust-trim` job is named `Cache`, installs the checksum-verified released helper, checks upload results, prunes old entries and enforces the repository cache budget. It has `actions: write` and `pull-requests: read`, no checkout and no repository code. It needs every job that can save a cache and runs on every default-branch `push`, `schedule` and `workflow_dispatch` event, including a run in which no lane saved. Failed verification, listing or deletion makes Cache red. Required does not depend on Cache. A source-helper repository without a released helper of at least 0.1.81 saves no base because it cannot safely run privileged trimming.

The test lane owns Sources saves, or the lint lane when no tests exist. Every root's lane restores and may save the same Sources key of the repository, so one payload per host serves all roots instead of one per root. Because a cache entry is immutable per key, the lane that saves the key must hold the crates of every root. Before the save, on a Sources miss that `cache decide` authorised, the lane fetches the crates of every Cargo root (fetch only, no build). With a pinned `velnor-ci` of 0.1.86 or later that is one step, `velnor-ci sources fetch --roots . ./<dir>...`, which runs `cargo fetch --locked` in each root's directory in the order listed through the same capture as `velnor-ci run`, and stops at the first failure. Older helpers get one `velnor-ci run -- cargo fetch --locked` step per root, each after the previous one succeeded, so the workflow grows with the square of the root count and generation refuses a repository with more than 16 Cargo roots while Sources are saved. In both forms the save requires the last fetch to have succeeded; a failed fetch saves nothing. Lanes that miss together race on one key and the loser's save writes nothing. The newest shared payload is still restored by the `all.roots-` prefix when a lockfile changed. The owner token `all.roots` holds a `.`, which a root slug never holds, so the shared key and its prefix neither equal nor prefix any key of another family or of a per-root `velnor-src-<host>-<root>-` key. An exact Sources hit is not uploaded again. Target and Sources families retain their newest two entries. Listing is paginated in batches of 100, with a bound of 20 pages and a warning if further pages remain.

Generation suppresses Target and Sources saves for non-crates.io dependencies or applicable Cargo source configuration. Runtime admission rechecks the explicitly supplied Cargo root manifests, their locks including unused patches, applicable ancestor Cargo configurations and `CARGO_HOME`. Missing, unreadable or unmodelled sources and source-related environment overrides disable both writes. Unrelated fixture lockfiles and descendant configurations do not affect admission.

Target paths cannot overlap tracked checkout content or repository control directories. Runtime validation also rejects paths outside the checkout and paths through symlinks. The generator supplies configured targets in `VELNOR_CACHE_TARGETS`; `restored` uses those exact directories for expiry and cold cleanup after failed timestamp validation. Inputs restore receives the same configured target path.

A restored base more than seven days old, or with missing or unreadable `.velnor/born`, is removed on the default branch and reports `expired=true`. The following `replan` step selects everything, and subsequent commands read the full plan. A fresh base retains its partial selection. The limit is passed as `VELNOR_BASE_MAX_AGE_DAYS`.

## Compat

`compat` is the first 16 hex characters of the SHA-256 of, in this order: the schema number (`COMPAT_SCHEMA`, bumped by hand when the layout of a cache changes), `rustc -vV`, `CARGO_BUILD_TARGET`, the CI build profile, `RUSTFLAGS`, the absolute workspace path, the build driver, the `[profile]` tables of the cargo configs that apply to a Cargo root and the runner image (`ImageOS` with `ID` and `VERSION_ID` of `/etc/os-release`). `velnor-ci prepare` gathers them and writes the result as a step output.

The cargo configs are found the way cargo finds them, not by walking the checkout. For each Cargo root recorded in `.velnor/rust-metadata.json`; `prepare` fails when the file is absent, `prepare` reads `.cargo/config` and `.cargo/config.toml` in the root's directory and in each ancestor up to the checkout root, and nothing else; `.cargo/config` shadows `.cargo/config.toml` in the same folder. A config in `target/`, `vendor/`, `node_modules` or a nested crate that is not on that path is never read. Git is asked which of those files are tracked, and that answer is used to compare `build.target-dir` as read now with the same key read from the tracked files, and `build.build-dir` the same way. `build.target-dir` read now is also compared with the path recorded as `target_dir` for the root in `.velnor/rust-metadata.json` at generation (recorded when the helper pin is 0.1.85 or later, the first release that reads it; a file without it skips this comparison): `prepare` fails naming both paths when a committed change to `build.target-dir` was made without regenerating. When git cannot answer, none of them count as tracked. A path that is a symbolic link, or that goes through one, makes `prepare` fail with an error naming the file, because generation does not read through a symbolic link. A top-level `include` key in a config that applies (not one shadowed by `.cargo/config`) makes `prepare` fail with the same error generation reports. Not modelled: folders above the checkout (which cargo also searches) and `CARGO_HOME`; a `[profile]` set there does not change `compat`.

## CI build profile

`velnor-ci prepare` appends these lines to `$GITHUB_ENV`, the same in every lane:

- `CARGO_INCREMENTAL=0`, `CARGO_TERM_COLOR=always`, `SOURCE_DATE_EPOCH=0`
- `CARGO_PROFILE_DEV_DEBUG` and `CARGO_PROFILE_TEST_DEBUG` set to `ci_debuginfo` (`full`, `line-tables-only` or `none`, default `line-tables-only`) for workspace crates

Cargo has no environment variable for the dependency override `[profile.<name>.package."*"]`. For `line-tables-only` and `none`, `prepare` writes it to `.cargo/config.toml` in the parent folder of the checkout, so it applies to every cargo command, including `cargo-nextest` and `cargo-clippy`. The file is Velnor's own: its first line is `# velnor-ci owns this file and rewrites it on every prepare`, `prepare` replaces it on every run and removes it when `ci_debuginfo` is `full`, and `prepare` fails when it needs the override and a file there does not carry that line. A `.cargo/config.toml` inside the repository takes precedence over it, which is why its `[profile]` tables are part of `compat`. Before Rust 1.71, which rejects string `debug` values, `prepare` writes `2`, `1` and `0` for `full`, `line-tables-only` and `none` and `debug = 0` for dependencies. Every action the generated workflow pins runs on node24, so a runner older than 2.327.1 stops at the first step, before any cache step. The override written for Rust 1.71 and later:

```toml
[profile.dev.package."*"]
debug = "none"

[profile.test.package."*"]
debug = "none"
```

## Cache telemetry and enforcement

With helper `0.1.81` or a supported source-built helper, each reporting-enabled
Cargo lane ends with `velnor-ci report` and uploads `ci-metrics.json`. These
steps also run after an earlier failure unless the run was cancelled. Report
and upload failures do not replace the lane's build result. The report retains
logs, provenance and step results, and emits local metrics without calling the
API or using `GITHUB_TOKEN`.
The workflow supplies each cache's family, root, and restore/save step IDs; the
helper derives observed facts from step results and filesystem evidence. When a
released bootstrap helper is older than `0.1.81`, the source-built artifact helper runs the
tokenless local checks and diagnostics without API access.

The separate `Cache` job checks the same-run metrics from every reporting
lane. With released helper `0.1.81` or later, it runs `report --check` to add
Actions API timings and cache archive sizes, then fails on cache waste, missing
metrics, or unverifiable required evidence. The job has `actions: read` for
this enrichment and receives an empty explicit API token on fork pull requests.
It has no checkout and runs no Cargo commands. It is advisory by default. To
include it in Required's dependencies and verdict specification, set this in
`.velnor/rust-ci.toml`:

```toml
cache_check_blocks = true
```

`rust-trim` continues to verify cache uploads and prune old entries without a
checkout or repository code. The `Cache` job needs no additional permission to
download artifacts from its own run. The schema-1 JSON separates cache
outcomes, captured Cargo invocations, display fields, verification errors, and
optional diagnostics.

## Worked example

A repository with the root `lab/Cargo.toml` and the job `rust-lint`, on `ubuntu-latest` (X64), at commit `c0ffee`, run `42`, compat `9f3a1c5e7b2d4a60`:

- Sources: `velnor-src-Linux-X64-lab-<hash of lab/Cargo.lock>`
- Target: `velnor-tgt-rust-lint.lab.Linux-X64-9f3a1c5e7b2d4a60-<hash of lab/Cargo.lock>-c0ffee-42`
- Restore tries the Target key, then `velnor-tgt-rust-lint.lab.Linux-X64-9f3a1c5e7b2d4a60-<hash of lab/Cargo.lock>-` (the latest save for this lockfile), then `velnor-tgt-rust-lint.lab.Linux-X64-9f3a1c5e7b2d4a60-` (the latest save for this toolchain and profile).
- After a dependency bump the lock hash changes: the first two tries miss and the third restores the previous build directory. The workflow file is unchanged.

The new generator supports a runner-scoped [optional crates mirror](../generator/CONFIG.md).

## Repository cache budget

`cache_budget_bytes` in `.velnor/rust-ci.toml` defaults to `8000000000` and accepts values from zero through that 8 GB maximum. A pinned released helper of 0.1.81 or later runs `velnor-ci cache budget --budget-bytes <bytes>` once as the final step of `rust-trim`, after all cache writers, the per-lane trims and the upload check. The job runs only on the default branch for push, schedule and manual events. Its budget step runs even when no lane saved, including a warm scheduled run. A repository with private dependencies still gets this budget step after its Tools writers, even when no Target or Sources trims exist. A source-built helper without a released helper of 0.1.81 or later gets no privileged cleanup job. It needs `actions: write` and `pull-requests: read`, installs the released helper without checking out repository code, and stays outside the required check.

Run `velnor-ci cache budget --dry-run --repo owner/name` to inspect the repository without deleting anything. `--budget-bytes` accepts a lower budget, including zero, up to the command’s 8 GB default. The command lists every cache and open pull request before deleting. An incomplete, inconsistent or failed listing aborts deletion. A PR absent from the open listing is checked individually before its delta is treated as closed. The table shows each family's count, bytes and newest and oldest creation timestamps. The report includes selected IDs, current bytes and projected remaining bytes.

Closed-PR deltas are always removed, even below budget. While over budget, deletion proceeds through deltas idle for at least three days, Target and Sources entries beyond the newest two per key group, Tools entries idle for at least seven days except the newest per host, then remaining deltas in access-time order. A delta is eligible only in a `refs/pull/<number>/merge` scope. Base groups are scoped to their Git ref, so PR entries cannot displace a default-branch base. Target groups retain job, root, host and compat; The Sources group retains the host alone. Tools versions share a host group. The newest base entries and unknown keys remain protected. If protected entries prevent reaching the budget, the report warns that retained bytes still exceed it. Saves never wait for a live usage query.

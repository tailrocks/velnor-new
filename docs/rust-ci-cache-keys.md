# Rust CI cache keys

`velnor-model` holds one type, `CacheKey`, for every cache entry of the generated Rust workflow. `velnor-render` has the only function that turns it into text (`render_key`). Every part that changes from commit to commit is an expression that GitHub fills in at run time, so a dependency bump changes no byte of the workflow file.

`host` is `${{ runner.os }}-${{ runner.arch }}`. `root` is the root's directory lower-cased, with every character other than an ASCII letter or digit turned into `_` (`root` for the repository root). `job` is the job id. `compat` is `steps.prepare.outputs.compat`.

| Family | Key | Restore fallbacks, in order | Path |
|---|---|---|---|
| Tools | `velnor-tools-<host>-mise<version>-${{ hashFiles('mise.lock') }}` | none | `~/.local/share/mise`, `~/.rustup` |
| Sources | `velnor-src-<host>-<root>-${{ hashFiles('<dir>/Cargo.lock') }}` | `velnor-src-<host>-<root>-` | `~/.cargo/registry/index`, `~/.cargo/registry/cache`, `~/.cargo/git/db` |
| Target | `velnor-tgt-<job>.<root>.<host>-<compat>-<lockhash>-${{ github.sha }}-${{ github.run_id }}` | `...-<compat>-<lockhash>-`, then `...-<compat>-` | `build.target-dir`, default `<dir>/target` |
| Delta | `velnor-dlt-pr<PR number>-<job>.<root>.<host>-<compat>-<base8>-${{ github.run_id }}` | `...-<compat>-<base8>-` | `<target-dir>.velnor-delta` |

`<lockhash>` is `${{ hashFiles('<dir>/Cargo.lock') }}` and `<base8>` is `${{ steps.delta_base_<root>.outputs.base8 }}`, the first eight hex characters of the SHA-256 of the target restore's actual `cache-matched-key`. A fallback is always a strict prefix of its key, and the second fallback of a Target key is a strict prefix of the first. Every Delta fallback retains the explicit PR number, lane, root, host, compat and base digest. A key never contains a branch name, a literal digest or `run_attempt`. `hashFiles` reads a root path literally: `*`, `?` and `[` are written as `[*]`, `[?]` and `[[]`, and a leading `!` or `#` gets a `./` prefix. Generation fails when a key can pass 512 characters at run time (the `actions/cache` limit), counting every expression at its longest value. `build.target-dir` is read from the `.cargo/config.toml` or `.cargo/config` that applies to the root (the nearest wins, the file without `.toml` first); generation fails when it is absolute, outside the repository or holds the root. Generated jobs set no `CARGO_TARGET_DIR`, and `[env]` entries starting with `CARGO` are refused. The run id at the end of a Target key lets a full run save a clean base when the commit has not changed since the last save. A rerun of the same run reuses that id on purpose: the save finds the key taken and writes nothing, so unchanged data is never saved twice, and a rerun after a failed save still saves because no entry exists yet.

## Tools cache

Every job that installs the tools restores the Tools entry in the step before `jdx/mise-action` (a shard job, which installs only cargo-nextest, restores it too; the plan job installs nothing and restores nothing). `~/.local/share/mise` holds the mise program itself (`bin/mise`, which the action finds and only verifies against the signed checksums, so it downloads nothing), the installed tools and the link to the Rust toolchain; `~/.rustup` holds the toolchain, because mise installs Rust through rustup. Rust components and targets are added in every job before the save, so every job saves the same content.

Only a job that installs every tool saves it, only on the default branch and only when the restore was not an exact hit. A tool-set change on a pull request installs fresh for that pull request and saves nothing. The key covers `mise.lock`, the mise program version the generator passes to `jdx/mise-action` as `version:`, the runner OS and the runner architecture: a changed tool version or a mise bump moves it (the entry is saved only on a miss, so without the mise version a bump would keep restoring a directory built by the old mise), a comment in `mise.toml` does not. It does not cover the runner image version or the targets in `rust-toolchain.toml`; a change in either only causes a re-download, never a wrong tool. Generation fails when `mise.lock` is missing, or when a pinned tool has no prebuilt binary for the runner's platform in it (a `cargo:` tool, for example); the error names the tool.

## Saving the build directory

With a helper of 0.1.81 or later, the lint and test lanes save the Target entry themselves, and no other job builds Rust to fill a cache. A lane runs these steps after its test build and before any test executes, each with `!cancelled()`, a successful `velnor-ci restored` and a ref that is the default branch on a `push`, `schedule` or `workflow_dispatch` event. A pull request, a merge group and a fork never save here. The lane splits `cargo nextest run` into `--no-run`, which builds every test binary, and the run, so the save costs no second build and a test cannot write into the saved directory. A lane without nextest (custom harnesses only) has no separate build step and saves after its tests; the lint lane, which runs no tests, saves after its last cargo step:

1. `velnor-ci inputs save`, in the root's directory, writes `target/.velnor/inputs.tsv`.
2. `velnor-ci cache decide` (step id `base`) reads `velnor-run.log`. It writes the output `save=true` only when the run compiled, checked or documented at least one crate; a fallback hit alone is no reason to save. With `true` it removes every `incremental` directory below the target directory and the nextest archive, writes `target/.velnor/born` with today's date when the file is missing, and logs the size of the target directory.
3. `actions/cache/save` of the Target key, when `steps.base.outputs.save == 'true'`. The action compresses the directory; no archive is nested in it.

Every lane job keeps `contents: read`, because pull request code runs in it. It publishes the job outputs `base_save` (the decision) and `base_prefix` (the Target key up to and including `<compat>-`). The separate job `rust-trim` then runs `velnor-ci cache trim` per lane whose `base_save` is `true`. It `needs` the lanes and runs only with `!cancelled()` on the default branch for a `push`, `schedule` or `workflow_dispatch` event, so never for a pull request or a merge group. It has the permission `actions: write` and no other, no checkout and no repository code: it installs the pinned released `velnor-ci` (or downloads the helper that the run built from the repository's `helper` key) and deletes. `trim` keeps the newest two entries on the default branch whose key starts with `base_prefix` and deletes the others through the API. A missing `GITHUB_TOKEN` or a failed API call prints a warning and exits 0, so a trim problem never fails the job. The `required` job does not wait for `rust-trim`.

The test lane (the job that runs the tests, or the lint lane when the repository has none) also saves the Sources entry when its restore was not an exact hit. The tools entry is saved by the step described above.

`target/.velnor/born` carries the date of the cold build forward through every later save. On an event that selects everything on the default branch (a schedule or a dispatch, and a push when `on_main = "everything"` or when no scheduled run exists), `velnor-ci restored` removes a restored target directory whose `born` date is more than 7 days old, or that has no `born` file (a base saved before the file existed), so that run builds cold and saves a clean base. The generator passes the limit as `VELNOR_BASE_MAX_AGE_DAYS`.

## Pull-request deltas

A locked helper of 0.1.81 or later enables delta caching for roots using their default `<root>/target` directory, including when lane jobs build their helper from source. Custom target directories receive no delta steps because the input manifest helper currently uses the default target path. After the Target restore, `velnor-ci delta prepare` fingerprints its matched key, the delta action restores the newest matching PR entry into staging, and `velnor-ci delta apply` copies its files over the target directory with their original modification times. The full base key saved inside the delta must equal the matched Target key; a missing base or different full key discards the delta. A failed overlay removes the restored target and builds cold. Symlinks are refused. Input restoration follows the overlay, so the delta's input manifest replaces the base manifest before source timestamps are restored. The existing `velnor-ci restored` then breaks timestamp ties.

After the lane's last compiling command, `inputs save` rewrites the manifest and `delta stage` carries forward the restored delta, adds target files modified since the overlay marker, removes staged files absent from the target, and always includes the input manifest. Copies preserve modification times in both directions. Incremental directories and delta bookkeeping are excluded. A run with no workspace crate compilation saves nothing. A staged payload over 500,000,000 bytes reports its size in a notice and saves nothing; its size is checked before copying. A cold run without a matched base also saves nothing.

Only `pull_request` runs restore deltas. A same-repository PR may save only its own Delta key. Fork PRs restore only. Main and merge-group runs never restore or save deltas. Lane jobs have no `actions: write` permission.

The generated `rust-cache-trim.yml` runs after Rust CI completes, using the default branch's workflow and locked released helper. Its sole `rust-trim` job checks out nothing, downloads no build artifacts, restores no cache and runs no project code. The helper validates the completed run's workflow, repository and same-repository PR identities before listing `refs/pull/N/merge` deltas. It reads every page before deleting, refuses changing totals, duplicate IDs or other refs, and keeps the newest entry per lane/root/host across compat and base generations. Cleanup runs serialize without cancellation. There can be multiple entries until trusted cleanup completes. Closed-PR cleanup and the aggregate 8 GB budget require the separate cache budget manager; this per-PR cleanup does not enforce them.

## Compat

`compat` is the first 16 hex characters of the SHA-256 of, in this order: the schema number (`COMPAT_SCHEMA`, bumped by hand when the layout of a cache changes), `rustc -vV`, `CARGO_BUILD_TARGET`, the CI build profile, `RUSTFLAGS`, the absolute workspace path, the build driver, the `[profile]` tables of the repository's cargo configs and the runner image (`ImageOS` with `ID` and `VERSION_ID` of `/etc/os-release`). `velnor-ci prepare` gathers them and writes the result as a step output.

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

## Worked example

A repository with the root `lab/Cargo.toml` and the job `rust-lint`, on `ubuntu-latest` (X64), at commit `c0ffee`, run `42`, compat `9f3a1c5e7b2d4a60`:

- Sources: `velnor-src-Linux-X64-lab-<hash of lab/Cargo.lock>`
- Target: `velnor-tgt-rust-lint.lab.Linux-X64-9f3a1c5e7b2d4a60-<hash of lab/Cargo.lock>-c0ffee-42`
- Restore tries the Target key, then `velnor-tgt-rust-lint.lab.Linux-X64-9f3a1c5e7b2d4a60-<hash of lab/Cargo.lock>-` (the latest save for this lockfile), then `velnor-tgt-rust-lint.lab.Linux-X64-9f3a1c5e7b2d4a60-` (the latest save for this toolchain and profile).
- After a dependency bump the lock hash changes: the first two tries miss and the third restores the previous build directory. The workflow file is unchanged.

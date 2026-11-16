# Rust CI cache keys

`velnor-model` holds one type, `CacheKey`, for every cache entry of the generated Rust workflow. `velnor-render` has the only function that turns it into text (`render_key`). Every part that changes from commit to commit is an expression that GitHub fills in at run time, so a dependency bump changes no byte of the workflow file.

`host` is `${{ runner.os }}-${{ runner.arch }}`. `root` is the root's directory lower-cased, with every character other than an ASCII letter or digit turned into `_` (`root` for the repository root). `job` is the job id. `compat` is `steps.prepare.outputs.compat`.

| Family | Key | Restore fallbacks, in order | Path |
|---|---|---|---|
| Tools | `velnor-tools-<host>-mise<version>-${{ hashFiles('mise.lock') }}` | none | `~/.local/share/mise`, `~/.rustup` |
| Sources | `velnor-src-<host>-<root>-${{ hashFiles('<dir>/Cargo.lock') }}` | `velnor-src-<host>-<root>-` | `~/.cargo/registry/index`, `~/.cargo/registry/cache`, `~/.cargo/git/db` |
| Target | `velnor-tgt-<job>-<root>-<host>-<compat>-<lockhash>-${{ github.sha }}-${{ github.run_id }}` | `...-<compat>-<lockhash>-`, then `...-<compat>-` | `build.target-dir`, default `<dir>/target` |
| Delta | `velnor-dlt-<job>-<root>-<host>-<compat>-<base8>-${{ github.run_id }}` | `...-<compat>-<base8>-` | same as Target |

`<lockhash>` is `${{ hashFiles('<dir>/Cargo.lock') }}` and `<base8>` is `${{ steps.restored.outputs.base8 }}`. A fallback is always a strict prefix of its key, and the second fallback of a Target key is a strict prefix of the first. A key never contains a branch name, a literal digest or `run_attempt`. `hashFiles` reads a root path literally: `*`, `?` and `[` are written as `[*]`, `[?]` and `[[]`, and a leading `!` or `#` gets a `./` prefix. Generation fails when a key can pass 512 characters at run time (the `actions/cache` limit), counting every expression at its longest value. `build.target-dir` is read from the `.cargo/config.toml` or `.cargo/config` that applies to the root (the nearest wins, the file without `.toml` first); generation fails when it is absolute, outside the repository, holds the root, starts with `~`, or contains a `{` or `}` placeholder, and when `build.build-dir` is set, because the Target cache would not cover that directory. Generated jobs set no `CARGO_TARGET_DIR`, and `[env]` entries starting with `CARGO` are refused. The run id at the end of a Target key lets a full run save a clean base when the commit has not changed since the last save. A rerun of the same run reuses that id on purpose: the save finds the key taken and writes nothing, so unchanged data is never saved twice, and a rerun after a failed save still saves because no entry exists yet.

## Tools cache

Every job that installs the tools restores the Tools entry in the step before `jdx/mise-action` (a shard job, which installs only cargo-nextest, restores it too; the plan job installs nothing and restores nothing). `~/.local/share/mise` holds the mise program itself (`bin/mise`, which the action finds and only verifies against the signed checksums, so it downloads nothing), the installed tools and the link to the Rust toolchain; `~/.rustup` holds the toolchain, because mise installs Rust through rustup. Rust components and targets are added in every job before the save, so every job saves the same content.

Only a job that installs every tool saves it, only on the default branch and only when the restore was not an exact hit. A tool-set change on a pull request installs fresh for that pull request and saves nothing. The key covers `mise.lock`, the mise program version the generator passes to `jdx/mise-action` as `version:`, the runner OS and the runner architecture: a changed tool version or a mise bump moves it (the entry is saved only on a miss, so without the mise version a bump would keep restoring a directory built by the old mise), a comment in `mise.toml` does not. It does not cover the runner image version or the targets in `rust-toolchain.toml`; a change in either only causes a re-download, never a wrong tool. Generation fails when `mise.lock` is missing, or when a pinned tool has no prebuilt binary for the runner's platform in it (a `cargo:` tool, for example); the error names the tool.

## Compat

`compat` is the first 16 hex characters of the SHA-256 of, in this order: the schema number (`COMPAT_SCHEMA`, bumped by hand when the layout of a cache changes), `rustc -vV`, `CARGO_BUILD_TARGET`, the CI build profile, `RUSTFLAGS`, the absolute workspace path, the build driver, the `[profile]` tables of the cargo configs that apply to a Cargo root and the runner image (`ImageOS` with `ID` and `VERSION_ID` of `/etc/os-release`). `velnor-ci prepare` gathers them and writes the result as a step output.

The cargo configs are found the way cargo finds them, not by walking the checkout. For each Cargo root recorded in `.velnor/rust-metadata.json` (only the checkout root when the file is absent), `prepare` reads `.cargo/config` and `.cargo/config.toml` in the root's directory and in each ancestor up to the checkout root, and nothing else; `.cargo/config` shadows `.cargo/config.toml` in the same folder. A config in `target/`, `vendor/`, `node_modules` or a nested crate that is not on that path is never read. Git is not asked, so a tracked, untracked or ignored config is read alike. A symlinked config is followed only when its target resolves inside the checkout; a target outside it makes `prepare` fail with an error naming the file. Not modelled: folders above the checkout (which cargo also searches) and `CARGO_HOME`; a `[profile]` set there does not change `compat`.

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
- Target: `velnor-tgt-rust-lint-lab-Linux-X64-9f3a1c5e7b2d4a60-<hash of lab/Cargo.lock>-c0ffee-42`
- Restore tries the Target key, then `velnor-tgt-rust-lint-lab-Linux-X64-9f3a1c5e7b2d4a60-<hash of lab/Cargo.lock>-` (the latest save for this lockfile), then `velnor-tgt-rust-lint-lab-Linux-X64-9f3a1c5e7b2d4a60-` (the latest save for this toolchain and profile).
- After a dependency bump the lock hash changes: the first two tries miss and the third restores the previous build directory. The workflow file is unchanged.

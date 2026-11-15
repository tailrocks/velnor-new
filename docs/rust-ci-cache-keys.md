# Rust CI cache keys

`velnor-model` holds one type, `CacheKey`, for every cache entry of the generated Rust workflow. `velnor-render` has the only function that turns it into text (`render_key`). Every part that changes from commit to commit is an expression that GitHub fills in at run time, so a dependency bump changes no byte of the workflow file.

`host` is `${{ runner.os }}-${{ runner.arch }}`. `root` is the root's directory lower-cased, with every character other than an ASCII letter or digit turned into `_` (`root` for the repository root). `job` is the job id. `compat` is `steps.prepare.outputs.compat`.

| Family | Key | Restore fallbacks, in order | Path |
|---|---|---|---|
| Tools | `velnor-tools-<host>-${{ hashFiles('mise.lock') }}` | none | `~/.local/share/mise`, `~/.rustup` |
| Sources | `velnor-src-<host>-<root>-${{ hashFiles('<dir>/Cargo.lock') }}` | `velnor-src-<host>-<root>-` | `~/.cargo/registry/index`, `~/.cargo/registry/cache`, `~/.cargo/git/db` |
| Target | `velnor-tgt-<job>-<root>-<host>-<compat>-<lockhash>-${{ github.sha }}-${{ github.run_id }}` | `...-<compat>-<lockhash>-`, then `...-<compat>-` | `<dir>/target` |
| Delta | `velnor-dlt-<job>-<root>-<host>-<compat>-<base8>-${{ github.run_id }}` | `...-<compat>-<base8>-` | `<dir>/target` |

`<lockhash>` is `${{ hashFiles('<dir>/Cargo.lock') }}` and `<base8>` is `${{ steps.restored.outputs.base8 }}`. A fallback is always a strict prefix of its key, and the second fallback of a Target key is a strict prefix of the first. A key never contains a branch name or a literal digest. The run id at the end of a Target key lets a full run save a clean base when the commit has not changed since the last save.

## Compat

`compat` is the first 16 hex characters of the SHA-256 of, in this order: the schema number (`COMPAT_SCHEMA`, bumped by hand when the layout of a cache changes), `rustc -vV`, `CARGO_BUILD_TARGET`, the CI build profile, `RUSTFLAGS`, the absolute workspace path and the build driver. `velnor-ci prepare` gathers them and writes the result as a step output.

## CI build profile

`velnor-ci prepare` appends these lines to `$GITHUB_ENV`, the same in every lane:

- `CARGO_INCREMENTAL=0`, `CARGO_TERM_COLOR=always`, `SOURCE_DATE_EPOCH=0`
- `CARGO_PROFILE_DEV_DEBUG` and `CARGO_PROFILE_TEST_DEBUG` set to `ci_debuginfo` (`full`, `line-tables-only` or `none`, default `line-tables-only`) for workspace crates

Cargo has no environment variable for the dependency override `[profile.<name>.package."*"]`. For `line-tables-only` and `none`, `prepare` writes it to `.cargo/config.toml` in the parent folder of the checkout, so it applies to every cargo command, including `cargo-nextest` and `cargo-clippy`. The file is Velnor's own: its first line is `# velnor-ci owns this file and rewrites it on every prepare`, `prepare` replaces it on every run and removes it when `ci_debuginfo` is `full`, and `prepare` fails when it needs the override and a file there does not carry that line. A `.cargo/config.toml` inside the repository takes precedence over it:

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

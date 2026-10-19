# Dependency Seed Contract

Status: proposed; specifies the `rsdeps` cache family and its only writer, the generated Cache seed
workflow. It extends the [cache and report contract](cache-contract.md).

## Family

The dependency seed holds the compiled registry and Git dependency units of one Cargo workspace root `R`
(`""` is the repository root), in the fixed lane profile directories the crate jobs use:

| Family | Paths | Restored by |
|---|---|---|
| `rsdeps` (schema 2) | `R/target/debug`, `R/target/velnor-clippy/debug`, `R/target/velnor-doc/debug` | every Rust crate job of `R`, read-only, before any task |
| `rsdeps-helper` (schema 2) | `target/release` | the bootstrap Plan job, right before `Build helper` (only when the policy builds the helper in Plan) |

Both are typed family specs (`workflow::cache_family`): every lookup, restore, and save takes the same
path list, and a path change requires a schema bump (pinned by test).

## Keys

`velnor-v2-<layer>-trusted-r<root12>-<compat>-<depset>`, where `root12` is the first 12 hex of
`sha256(R)`, so the janitor can delete one root's generations by prefix. The keys always carry `trusted`: no
pull-request dependency seed exists, and every run (push, pull request, merge group) restores the trusted
exact key with the trusted `...-r<root12>-<compat>-` prefix. Both digests are `s256-<64 hex>` values the
generated `Compute cache keys` step computes at run time (the Plan job, and each Cache seed job with the same
generated step):

- compat: the family, schema, and paths; `uname -s` and `uname -m`; `$ImageOS` (never `ImageVersion`); the
  absolute `$GITHUB_WORKSPACE` and Cargo home; `R`; the values of `RUSTFLAGS`, `RUSTDOCFLAGS`,
  `CARGO_ENCODED_RUSTFLAGS`, and `CARGO_BUILD_RUSTFLAGS`; and the installed toolchain's `rustc -vV`.
- depset: the `R/Cargo.lock` digest; the git blob IDs (`git ls-files -s`) of every tracked `Cargo.toml` under
  `R`, nested workspace roots excluded, and of `.cargo/config` and `.cargo/config.toml` under `R` and the
  repository root; and the seed lane records: the task ID, argv, and env of every build, Clippy, and doc
  record of `R`'s crates in `.github/velnor/task-execution-manifest-v1.json`, never the generator, helper,
  task, or execution version markers. The helper depset hashes the fixed Plan helper-build argv instead of
  lane records.

The Plan job publishes `rsdeps_key_<n>` and `rsdeps_restore_keys_<n>` per root ordinal `n`, and runs
`actions/cache/restore` with `lookup-only: true` on each exact key to publish `rsdeps_exact_<n>`, the
expected tier for cache health reporting.

## Cache seed workflow

`.github/workflows/cache-seed.yml` is generated whenever a lockful workspace root owns a Rust crate job.

- Trigger: `workflow_run` of the CI workflow, `types: [completed]`. Each job runs only when the triggering
  run was a push to the default branch from this repository and was not cancelled (decision 1: a red run
  still seeds).
- Concurrency: its own group `velnor-cache-seed-<head branch>`, `cancel-in-progress: false` (GitHub keeps
  one pending run; the newest wins). It never holds CI's group, and CI never waits for it. It is not part
  of Required.
- Permissions: `contents: read` plus `actions: write` (cache deletion and the triggering run's helper
  artifact) at job level.
- One job per root, `timeout-minutes` from `[workflow] seed_timeout_minutes` (default 45, `1..=360`).

Each job checks out the triggering run's head commit (`persist-credentials: false`, depth 1) at the same
absolute path as the crate jobs, prepares the same tools, Rust components, job env, and install-only MBX as
the root's crate jobs, acquires the helper (consumers and lock-backed repositories as crate jobs do;
bootstrap repositories download `velnor-preseed-helper` from the triggering run and verify its manifest
against that run's head commit), and runs `Compute cache keys`. A lookup-only probe then skips every
remaining step when the exact key exists. Otherwise it:

1. restores the trusted sources snapshot and the previous generation by prefix (a warm seed), installs MBX,
   and fetches sources offline-first;
2. reads `cargo metadata --format-version 1 --locked --offline --all-features` and stubs every workspace
   member target source in place (`seed-skeleton-v1`): library-like targets get `//! Velnor dependency seed
   stub.`, every other target also `fn main() {}`. Path packages a registry or Git package depends on
   (`[patch]` targets) and their path-dependency closure keep their real sources. Manifests and
   `Cargo.lock` are never touched;
3. runs the build, Clippy, and doc lanes as three sequential steps (plus the helper lane in bootstrap
   repositories, the exact Plan helper-build argv). Each lane replays, one crate at a time, the exact
   manifest record of every crate of `R` for that kind (`resolve-seed-task-v1`, selected by execution
   digest, validated like `resolve-task-execution-v1` without a plan binding), with machine-output flags
   inserted after the Cargo subcommand (`--cargo-message-format json-render-diagnostics` for `nextest
   list`; `--message-format json-render-diagnostics --keep-going` for Clippy and doc) and
   `CARGO_BUILD_JOBS` set to `[resources] compiler_process_budget`. A failing crate is recorded, the lane
   continues, and the lane step fails at its end;
4. trims every lane profile (`seed-trim-v1`): only the units this run reported (`compiler-artifact`, fresh
   or not, and `build-script-executed`) of registry and Git packages whose dependency closure reaches no
   path package keep their `deps/` files, `build/` directories, and `.fingerprint/` directories; members,
   path and tainted units, executables, `incremental`, `examples`, `.cargo-lock`, symlinks, and units left
   over from the previous generation are deleted. The trim refuses to run while a compiler process is alive
   and writes `seed-report.json` (per-lane kept, dropped, compiled, and fresh counts and duration; per-family
   completeness and raw bytes);
5. saves one complete generation: `always() && !cancelled()`, the exact key absent, every lane and the trim
   succeeded, and the raw bytes at or below `[workflow] seed_max_bytes` (default 8 GiB; above it the trim
   warns `seed: over_ceiling` and nothing is saved). There are no partial saves;
6. deletes every other generation of that family and root on the default branch (pinned `gh` through Mise,
   the job token bound at render time), keeping one complete generation.

## Budget

Generation fails unless the projected hot set stays at or below 85% of `[workflow] cache_quota_bytes`
(default 10 GB): 0.4 GB per written tools identity, two 0.15 GB sources generations, `seed_max_bytes / 4`
per seeded root and family, and 0 for member bundles until they exist. The quarter assumes the measured
3.8-4.4x zstd ratio of dependency seed archives (the cache service stores the compressed archive).

## Doctor

The generation-time cache doctor checks every dependency seed step: keys come only from the key step or the
Plan outputs; paths are the family spec list of one valid root; restores pair a key with the same root's
prefix and lookups are `lookup-only`; no other family owns or nests those paths; and only the Cache seed
workflow saves, under the exact producer gate. Seed-only roles (lanes, trim, skeleton, janitor, saves) are
rejected in every other workflow.
